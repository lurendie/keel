<script setup lang="ts">
import { computed, onMounted, reactive, ref } from "vue";
import {
  CopyDocument,
  Download,
  MagicStick,
  Refresh,
} from "@element-plus/icons-vue";
import { ElMessage } from "element-plus";
import YAML from "yaml";
import PageIntro from "../components/PageIntro.vue";
import { listProjects, type Project } from "../api/api";

type RuntimeKind =
  | "node"
  | "rust"
  | "go"
  | "python"
  | "java-maven"
  | "java-gradle"
  | "custom";

type RunnerKind = "ubuntu-latest" | "windows-latest" | "macos-latest";
type TriggerKind = "tag" | "manual";
type ArchiveKind = "zip" | "tar.gz" | "file";

interface WorkflowForm {
  projectId: number | null;
  workflowName: string;
  runner: RunnerKind;
  trigger: TriggerKind;
  tagPattern: string;
  branch: string;
  runtime: RuntimeKind;
  runtimeVersion: string;
  installCommand: string;
  buildCommand: string;
  artifactSource: string;
  archiveFormat: ArchiveKind;
  artifactName: string;
  generateReleaseNotes: boolean;
  prerelease: boolean;
  draft: boolean;
  notifyDeploy: boolean;
}

const projects = ref<Project[]>([]);
const loadingProjects = ref(false);
const copied = ref(false);

const form = reactive<WorkflowForm>({
  projectId: null,
  workflowName: "Keel Release and Deploy",
  runner: "ubuntu-latest",
  trigger: "tag",
  tagPattern: "v*",
  branch: "main",
  runtime: "node",
  runtimeVersion: "22",
  installCommand: "npm ci",
  buildCommand: "npm run build",
  artifactSource: "dist",
  archiveFormat: "zip",
  artifactName: "app.zip",
  generateReleaseNotes: true,
  prerelease: false,
  draft: false,
  notifyDeploy: true,
});

const runtimeOptions: Array<{ label: string; value: RuntimeKind }> = [
  { label: "Node.js", value: "node" },
  { label: "Rust", value: "rust" },
  { label: "Go", value: "go" },
  { label: "Python", value: "python" },
  { label: "Java / Maven", value: "java-maven" },
  { label: "Java / Gradle", value: "java-gradle" },
  { label: "自定义", value: "custom" },
];

const runnerOptions: Array<{ label: string; value: RunnerKind }> = [
  { label: "Ubuntu", value: "ubuntu-latest" },
  { label: "Windows", value: "windows-latest" },
  { label: "macOS", value: "macos-latest" },
];

const selectedProject = computed(() =>
  projects.value.find((project) => project.id === form.projectId),
);

const selectedRepository = computed(() => {
  const repository = selectedProject.value?.repo_path || "";
  return repository.includes("/") && !repository.includes("\\")
    ? repository.replace(/\.git$/, "")
    : "GitHub 仓库会自动使用当前仓库";
});

const generatedYaml = computed(() => buildWorkflowYaml(form));

const loadProjects = async () => {
  loadingProjects.value = true;
  try {
    projects.value = await listProjects();
  } catch (error) {
    ElMessage.error(error instanceof Error ? error.message : "项目加载失败");
  } finally {
    loadingProjects.value = false;
  }
};

const selectProject = (projectId: number | null) => {
  if (!projectId) {
    return;
  }

  const project = projects.value.find((item) => item.id === projectId);
  if (!project) {
    return;
  }

  form.workflowName = `${project.name} Release`;
  form.branch = project.branch;
  applyRuntimePreset(inferRuntime(project.build_command));
};

const applyRuntimePreset = (runtime: RuntimeKind) => {
  form.runtime = runtime;

  const presets: Record<RuntimeKind, Partial<WorkflowForm>> = {
    node: {
      runtimeVersion: "22",
      installCommand: "npm ci",
      buildCommand: "npm run build",
      artifactSource: "dist",
      archiveFormat: "zip",
      artifactName: "app.zip",
    },
    rust: {
      runtimeVersion: "stable",
      installCommand: "",
      buildCommand: "cargo build --release",
      artifactSource: "target/release/app",
      archiveFormat: "file",
      artifactName: "app",
    },
    go: {
      runtimeVersion: "1.23",
      installCommand: "go mod download",
      buildCommand: "go build -o app .",
      artifactSource: "app",
      archiveFormat: "file",
      artifactName: "app",
    },
    python: {
      runtimeVersion: "3.12",
      installCommand: "pip install -r requirements.txt",
      buildCommand: "python -m build",
      artifactSource: "dist",
      archiveFormat: "tar.gz",
      artifactName: "app.tar.gz",
    },
    "java-maven": {
      runtimeVersion: "21",
      installCommand: "",
      buildCommand: "./mvnw clean package -DskipTests",
      artifactSource: "target/app.jar",
      archiveFormat: "file",
      artifactName: "app.jar",
    },
    "java-gradle": {
      runtimeVersion: "21",
      installCommand: "",
      buildCommand: "./gradlew clean build -x test",
      artifactSource: "build/libs/app.jar",
      archiveFormat: "file",
      artifactName: "app.jar",
    },
    custom: {
      runtimeVersion: "",
      installCommand: "",
      buildCommand: "",
      artifactSource: "",
      archiveFormat: "file",
      artifactName: "app",
    },
  };

  Object.assign(form, presets[runtime]);
};

const inferRuntime = (command: string): RuntimeKind => {
  const value = command.toLowerCase();
  if (value.includes("pnpm") || value.includes("npm") || value.includes("yarn")) {
    return "node";
  }
  if (value.includes("cargo")) {
    return "rust";
  }
  if (value.includes("go build")) {
    return "go";
  }
  if (value.includes("mvn")) {
    return "java-maven";
  }
  if (value.includes("gradle")) {
    return "java-gradle";
  }
  if (value.includes("python") || value.includes("pip")) {
    return "python";
  }
  return "custom";
};

const buildWorkflowYaml = (config: WorkflowForm) => {
  const tagExpression =
    config.trigger === "manual" ? "${{ inputs.tag_name }}" : "${{ github.ref_name }}";
  const checkoutRef =
    config.trigger === "manual" ? "${{ inputs.tag_name || github.ref }}" : "${{ github.ref }}";
  const steps: Array<Record<string, unknown>> = [
    {
      name: "Checkout",
      uses: "actions/checkout@v4",
      with: { ref: checkoutRef },
    },
    ...runtimeSetupSteps(config),
  ];

  if (config.installCommand.trim()) {
    steps.push({
      name: "Install dependencies",
      run: config.installCommand.trim(),
    });
  }

  steps.push(
    {
      name: "Build",
      run: config.buildCommand.trim() || "echo 'No build command configured'",
    },
    {
      name: "Package release asset",
      shell: config.runner === "windows-latest" ? "pwsh" : "bash",
      run: packageCommand(config),
    },
    {
      name: "Create or update release",
      uses: "softprops/action-gh-release@v2",
      with: {
        tag_name: tagExpression,
        files: config.artifactName,
        generate_release_notes: config.generateReleaseNotes,
        prerelease: config.prerelease,
        draft: config.draft,
      },
    },
  );

  if (config.notifyDeploy) {
    steps.push(callbackStep(config, tagExpression));
  }

  const on =
    config.trigger === "manual"
      ? {
          workflow_dispatch: {
            inputs: {
              tag_name: {
                description: "Release tag",
                required: true,
                type: "string",
              },
            },
          },
        }
      : {
          push: {
            tags: [config.tagPattern.trim() || "v*"],
          },
        };

  return YAML.stringify(
    {
      name: config.workflowName.trim() || "Keel Release and Deploy",
      on,
      permissions: {
        contents: "write",
      },
      jobs: {
        release: {
          "runs-on": config.runner,
          steps,
        },
      },
    },
    { lineWidth: 0 },
  );
};

const runtimeSetupSteps = (config: WorkflowForm): Array<Record<string, unknown>> => {
  switch (config.runtime) {
    case "node":
      return [
        {
          name: "Setup Node.js",
          uses: "actions/setup-node@v4",
          with: {
            "node-version": config.runtimeVersion,
            cache: "npm",
          },
        },
      ];
    case "rust":
      return [
        {
          name: "Setup Rust",
          uses: "dtolnay/rust-toolchain@stable",
          with: { toolchain: config.runtimeVersion || "stable" },
        },
      ];
    case "go":
      return [
        {
          name: "Setup Go",
          uses: "actions/setup-go@v5",
          with: { "go-version": config.runtimeVersion },
        },
      ];
    case "python":
      return [
        {
          name: "Setup Python",
          uses: "actions/setup-python@v5",
          with: { "python-version": config.runtimeVersion },
        },
      ];
    case "java-maven":
    case "java-gradle":
      return [
        {
          name: "Setup Java",
          uses: "actions/setup-java@v4",
          with: {
            distribution: "temurin",
            "java-version": config.runtimeVersion,
          },
        },
      ];
    default:
      return [];
  }
};

const packageCommand = (config: WorkflowForm) => {
  const source = config.artifactSource.trim() || ".";
  const artifact = config.artifactName.trim() || "app";
  const checksumCommand = `if command -v sha256sum >/dev/null 2>&1; then\n  ARTIFACT_HASH=$(sha256sum "${artifact}" | awk '{print $1}')\nelse\n  ARTIFACT_HASH=$(shasum -a 256 "${artifact}" | awk '{print $1}')\nfi\necho "ARTIFACT_SHA=$ARTIFACT_HASH" >> "$GITHUB_ENV"`;

  if (config.archiveFormat === "file") {
    return config.runner === "windows-latest"
      ? `Copy-Item -LiteralPath "${source}" -Destination "${artifact}" -Force\n$hash = (Get-FileHash -Algorithm SHA256 "${artifact}").Hash.ToLower()\n"ARTIFACT_SHA=$hash" | Out-File -FilePath $env:GITHUB_ENV -Append`
      : `cp -f "${source}" "${artifact}"\n${checksumCommand}`;
  }

  if (config.archiveFormat === "tar.gz") {
    return config.runner === "windows-latest"
      ? `tar -czf "${artifact}" -C "${source}" .\n$hash = (Get-FileHash -Algorithm SHA256 "${artifact}").Hash.ToLower()\n"ARTIFACT_SHA=$hash" | Out-File -FilePath $env:GITHUB_ENV -Append`
      : `tar -czf "${artifact}" -C "${source}" .\n${checksumCommand}`;
  }

  return config.runner === "windows-latest"
    ? `Compress-Archive -Path "${source}\\*" -DestinationPath "${artifact}" -Force\n$hash = (Get-FileHash -Algorithm SHA256 "${artifact}").Hash.ToLower()\n"ARTIFACT_SHA=$hash" | Out-File -FilePath $env:GITHUB_ENV -Append`
    : `cd "${source}" && zip -r "../${artifact}" .\ncd ..\n${checksumCommand}`;
};

const callbackStep = (config: WorkflowForm, tagExpression: string) => ({
  name: "Notify Keel deployment",
  shell: "bash",
  env: {
    DEPLOY_BASE_URL: "${{ vars.AUTO_DEPLOY_BASE_URL }}",
    WEBHOOK_SECRET: "${{ secrets.AUTO_DEPLOY_SECRET }}",
    REPOSITORY: "${{ github.repository }}",
    BRANCH_NAME: config.branch.trim() || "main",
    TAG_NAME: tagExpression,
    COMMIT_SHA: "${{ github.sha }}",
    ARTIFACT_URL: `\${{ github.server_url }}/\${{ github.repository }}/releases/download/${tagExpression}/${config.artifactName}`,
    ARTIFACT_SHA: "${{ env.ARTIFACT_SHA }}",
  },
  run: `node <<'NODE'
const crypto = require("node:crypto");

(async () => {
  const payload = JSON.stringify({
    repository: process.env.REPOSITORY,
    branch: process.env.BRANCH_NAME,
    tag: process.env.TAG_NAME,
    commit: process.env.COMMIT_SHA,
    artifact_url: process.env.ARTIFACT_URL,
    artifact_sha256: process.env.ARTIFACT_SHA,
    message: "GitHub Actions release build",
  });
  const signature = crypto
    .createHmac("sha256", process.env.WEBHOOK_SECRET)
    .update(payload)
    .digest("hex");
  const response = await fetch(
    \`\${process.env.DEPLOY_BASE_URL}/api/deploy/callback\`,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Auto-Deploy-Signature": \`sha256=\${signature}\`,
      },
      body: payload,
    },
  );
  const body = await response.text();
  if (!response.ok) {
    throw new Error(\`deployment callback failed: \${response.status} \${body}\`);
  }
  console.log(body);
})().catch((error) => {
  console.error(error);
  process.exit(1);
});
NODE`,
});

const copyYaml = async () => {
  try {
    await navigator.clipboard.writeText(generatedYaml.value);
    copied.value = true;
    window.setTimeout(() => (copied.value = false), 1600);
    ElMessage.success("Workflow YAML 已复制");
  } catch {
    ElMessage.error("复制失败，请手动选择文本");
  }
};

const downloadYaml = () => {
  const blob = new Blob([generatedYaml.value], { type: "text/yaml;charset=utf-8" });
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = "release.yml";
  link.click();
  URL.revokeObjectURL(url);
};

onMounted(loadProjects);
</script>

<template>
    <div class="ci-generator-page">
        <PageIntro
            eyebrow="Workspace / CI generator"
            title="生成 Release 流水线"
            description="配置 GitHub Actions 的运行环境、构建方式和部署回调，生成可直接使用的 Release workflow。"
        >
            <template #actions>
                <el-button class="quiet-button" :loading="loadingProjects" @click="loadProjects">
                    <el-icon><Refresh /></el-icon>
                    读取项目
                </el-button>
            </template>
        </PageIntro>

        <section class="generator-content">
            <div class="generator-grid">
                <div class="generator-panel">
                    <div class="panel-heading">
                        <div class="panel-icon"><MagicStick /></div>
                        <div>
                            <span>Workflow options</span>
                            <h2>流水线参数</h2>
                        </div>
                    </div>

                    <el-form label-position="top" class="generator-form">
                        <div class="form-section">
                            <div class="section-title">触发方式</div>
                            <div class="form-grid two-columns">
                                <el-form-item label="关联项目">
                                    <el-select
                                        v-model="form.projectId"
                                        clearable
                                        placeholder="可选"
                                        style="width: 100%"
                                        @change="selectProject"
                                    >
                                        <el-option
                                            v-for="project in projects"
                                            :key="project.id"
                                            :label="project.name"
                                            :value="project.id"
                                        />
                                    </el-select>
                                </el-form-item>
                                <el-form-item label="Workflow 名称">
                                    <el-input v-model="form.workflowName" />
                                </el-form-item>
                                <el-form-item label="触发事件">
                                    <el-radio-group v-model="form.trigger">
                                        <el-radio-button value="tag">推送 Tag</el-radio-button>
                                        <el-radio-button value="manual">手动执行</el-radio-button>
                                    </el-radio-group>
                                </el-form-item>
                                <el-form-item v-if="form.trigger === 'tag'" label="Tag 规则">
                                    <el-input v-model="form.tagPattern" placeholder="v*" />
                                </el-form-item>
                                <el-form-item label="部署分支">
                                    <el-input v-model="form.branch" placeholder="main" />
                                </el-form-item>
                            </div>
                            <div class="repository-note">
                                <span>部署项目仓库标识</span>
                                <code>{{ selectedRepository }}</code>
                            </div>
                        </div>

                        <div class="form-section">
                            <div class="section-title">运行环境</div>
                            <div class="form-grid three-columns">
                                <el-form-item label="Runner 系统">
                                    <el-select v-model="form.runner" style="width: 100%">
                                        <el-option
                                            v-for="runner in runnerOptions"
                                            :key="runner.value"
                                            :label="runner.label"
                                            :value="runner.value"
                                        />
                                    </el-select>
                                </el-form-item>
                                <el-form-item label="运行时">
                                    <el-select
                                        :model-value="form.runtime"
                                        style="width: 100%"
                                        @change="applyRuntimePreset"
                                    >
                                        <el-option
                                            v-for="runtime in runtimeOptions"
                                            :key="runtime.value"
                                            :label="runtime.label"
                                            :value="runtime.value"
                                        />
                                    </el-select>
                                </el-form-item>
                                <el-form-item label="版本">
                                    <el-input
                                        v-model="form.runtimeVersion"
                                        placeholder="例如 22 / stable / 21"
                                    />
                                </el-form-item>
                            </div>
                        </div>

                        <div class="form-section">
                            <div class="section-title">构建与制品</div>
                            <div class="form-grid two-columns">
                                <el-form-item label="依赖安装命令">
                                    <el-input
                                        v-model="form.installCommand"
                                        placeholder="留空则不执行"
                                    />
                                </el-form-item>
                                <el-form-item label="构建命令">
                                    <el-input v-model="form.buildCommand" />
                                </el-form-item>
                                <el-form-item label="制品源路径">
                                    <el-input
                                        v-model="form.artifactSource"
                                        placeholder="dist / target/release/app"
                                    />
                                </el-form-item>
                                <el-form-item label="打包格式">
                                    <el-select v-model="form.archiveFormat" style="width: 100%">
                                        <el-option label="ZIP" value="zip" />
                                        <el-option label="TAR.GZ" value="tar.gz" />
                                        <el-option label="单文件，不打包" value="file" />
                                    </el-select>
                                </el-form-item>
                                <el-form-item label="Release 制品文件名">
                                    <el-input v-model="form.artifactName" placeholder="app.zip" />
                                </el-form-item>
                            </div>
                        </div>

                        <div class="form-section form-section-last">
                            <div class="section-title">Release 与部署</div>
                            <div class="switch-list">
                                <div class="switch-row">
                                    <div>
                                        <strong>自动生成 Release Notes</strong>
                                        <span>由 GitHub 根据提交记录生成</span>
                                    </div>
                                    <el-switch v-model="form.generateReleaseNotes" />
                                </div>
                                <div class="switch-row">
                                    <div>
                                        <strong>标记为预发布</strong>
                                        <span>适合 beta、rc 版本</span>
                                    </div>
                                    <el-switch v-model="form.prerelease" />
                                </div>
                                <div class="switch-row">
                                    <div>
                                        <strong>保存为草稿</strong>
                                        <span>草稿 Release 不会公开</span>
                                    </div>
                                    <el-switch v-model="form.draft" />
                                </div>
                                <div class="switch-row">
                                    <div>
                                        <strong>通知自动部署系统</strong>
                                        <span>Release 上传完成后调用部署回调</span>
                                    </div>
                                    <el-switch v-model="form.notifyDeploy" />
                                </div>
                            </div>
                            <div v-if="form.notifyDeploy" class="github-config">
                                <span>GitHub variables / secrets</span>
                                <code>vars.AUTO_DEPLOY_BASE_URL</code>
                                <code>secrets.AUTO_DEPLOY_SECRET</code>
                            </div>
                        </div>
                    </el-form>
                </div>

                <div class="generator-panel preview-panel">
                    <div class="preview-toolbar">
                        <div>
                            <span>Generated workflow</span>
                            <strong>.github/workflows/release.yml</strong>
                        </div>
                        <div class="preview-actions">
                            <el-button @click="copyYaml">
                                <el-icon><CopyDocument /></el-icon>
                                {{ copied ? "已复制" : "复制" }}
                            </el-button>
                            <el-button type="primary" @click="downloadYaml">
                                <el-icon><Download /></el-icon>
                                下载
                            </el-button>
                        </div>
                    </div>
                    <pre class="code-preview"><code>{{ generatedYaml }}</code></pre>
                </div>
            </div>
        </section>
    </div>
</template>

<style scoped>
.ci-generator-page {
    min-height: 100%;
    background: #ffffff;
}

.generator-content {
    padding: 28px 36px 44px;
}

.generator-grid {
    display: grid;
    grid-template-columns: minmax(0, 1fr) minmax(420px, 1.05fr);
    align-items: start;
    gap: 18px;
}

.generator-panel {
    min-width: 0;
    background: #ffffff;
    border: 1px solid #e5e5e5;
}

.generator-panel:first-child {
    padding: 24px;
}

.panel-heading {
    display: flex;
    align-items: center;
    gap: 12px;
    margin-bottom: 24px;
}

.panel-icon {
    display: grid;
    width: 38px;
    height: 38px;
    place-items: center;
    color: #262626;
    background: #f0f0f0;
}

.panel-icon :deep(svg) {
    width: 19px;
    height: 19px;
}

.panel-heading span,
.preview-toolbar span {
    color: #8a8a8a;
    font-size: 10px;
    font-weight: 750;
    letter-spacing: 0.12em;
    text-transform: uppercase;
}

.panel-heading h2 {
    margin: 5px 0 0;
    color: #171717;
    font-size: 18px;
    font-weight: 650;
}

.generator-form :deep(.el-form-item) {
    margin-bottom: 16px;
}

.generator-form :deep(.el-form-item__label) {
    height: auto;
    margin-bottom: 6px;
    padding: 0;
    color: #595959;
    font-size: 12px;
    line-height: 1.3;
}

.form-section {
    padding-bottom: 22px;
    border-bottom: 1px solid #ededed;
}

.form-section + .form-section {
    padding-top: 22px;
}

.form-section-last {
    padding-bottom: 0;
    border-bottom: 0;
}

.section-title {
    margin-bottom: 14px;
    color: #262626;
    font-size: 13px;
    font-weight: 700;
}

.form-grid {
    display: grid;
    gap: 0 14px;
}

.two-columns {
    grid-template-columns: repeat(2, minmax(0, 1fr));
}

.three-columns {
    grid-template-columns: 1fr 1fr 0.8fr;
}

.repository-note {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 14px;
    margin-top: 2px;
    padding: 10px 12px;
    background: #f7f7f7;
    font-size: 11px;
}

.repository-note span {
    color: #737373;
}

.repository-note code {
    overflow: hidden;
    color: #262626;
    font-size: 11px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.switch-list {
    display: grid;
    gap: 2px;
}

.switch-row {
    display: flex;
    min-height: 62px;
    align-items: center;
    justify-content: space-between;
    gap: 18px;
    padding: 12px 0;
    border-bottom: 1px solid #f0f0f0;
}

.switch-row:last-child {
    border-bottom: 0;
}

.switch-row > div {
    display: flex;
    min-width: 0;
    flex-direction: column;
}

.switch-row strong {
    color: #262626;
    font-size: 12px;
}

.switch-row span {
    margin-top: 4px;
    color: #8a8a8a;
    font-size: 11px;
}

.github-config {
    display: flex;
    align-items: center;
    gap: 7px;
    margin-top: 14px;
    padding: 10px 12px;
    flex-wrap: wrap;
    background: #f7f7f7;
}

.github-config span {
    margin-right: auto;
    color: #737373;
    font-size: 11px;
}

.github-config code {
    color: #262626;
    font-size: 10px;
}

.preview-panel {
    position: sticky;
    top: 18px;
    overflow: hidden;
}

.preview-toolbar {
    display: flex;
    min-height: 70px;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 14px 16px;
    border-bottom: 1px solid #e5e5e5;
}

.preview-toolbar > div:first-child {
    display: flex;
    min-width: 0;
    flex-direction: column;
}

.preview-toolbar strong {
    overflow: hidden;
    margin-top: 5px;
    color: #262626;
    font-family: "Cascadia Code", Consolas, monospace;
    font-size: 12px;
    text-overflow: ellipsis;
    white-space: nowrap;
}

.preview-actions {
    display: flex;
    flex-shrink: 0;
    gap: 8px;
}

.code-preview {
    height: calc(100vh - 245px);
    min-height: 520px;
    margin: 0;
    padding: 18px;
    overflow: auto;
    color: #262626;
    background: #f7f7f7;
    font-family: "Cascadia Code", "SFMono-Regular", Consolas, monospace;
    font-size: 11px;
    line-height: 1.65;
    tab-size: 2;
    white-space: pre;
}

.quiet-button {
    color: #404040;
    background: #f5f5f5;
    border: 1px solid #e2e2e2;
}

.quiet-button:hover {
    color: #171717;
    background: #ebebeb;
}

@media (max-width: 1100px) {
    .generator-grid {
        grid-template-columns: 1fr;
    }

    .preview-panel {
        position: static;
    }

    .code-preview {
        height: 620px;
    }
}

@media (max-width: 720px) {
    .generator-content {
        padding: 20px 18px 34px;
    }

    .generator-panel:first-child {
        padding: 18px;
    }

    .form-grid,
    .three-columns {
        grid-template-columns: 1fr;
    }

    .preview-toolbar {
        align-items: stretch;
        flex-direction: column;
    }

    .preview-actions {
        width: 100%;
    }

    .preview-actions :deep(.el-button) {
        flex: 1;
    }

    .repository-note {
        align-items: flex-start;
        flex-direction: column;
    }
}
</style>
