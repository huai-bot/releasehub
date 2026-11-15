# 把软件资料交给你的 Agent

ReleaseHub 提供软件信息，你在外部 Agent 平台完成分析、编译、修复或安装。它不内置 AI 聊天，不接管外部平台的执行或审批流程。

推荐使用 **MCP**：支持 Claude Code、Claude Desktop、Codex、Cursor、VS Code，以及能启动本机 stdio 程序的其他 MCP 客户端。这里的“支持”指连接格式和协议兼容，不要求你更换常用平台。纯云端连接不能直接访问这台电脑；远程开发窗口也须确认连接程序实际运行在 ReleaseHub 所在的 Windows 上。

## 第一次连接

1. 打开 ReleaseHub，点击左侧 **AI 连接**。
2. 打开 **开启软件资料连接**，使用期间保持 ReleaseHub 运行。
3. 选择自己的平台，点击 **复制配置**。没有专用选项时选 **通用 JSON**。
4. 在客户端的 MCP 设置里添加连接，保留已经存在的其他配置：

   | 平台格式 | 放在哪里 |
   | --- | --- |
   | 通用 JSON | 新建本机 stdio 连接，分别填入 `command` 和 `args`；连接名可用 `releasehub` |
   | Claude | Claude Code 的项目 MCP 配置，或 Claude Desktop 的 MCP 配置；合并 `mcpServers.releasehub` |
   | Codex | Codex 配置中的 `[mcp_servers.releasehub]` 段，不重复添加同名段 |
   | Cursor | Cursor 的 MCP 设置，合并 `mcpServers.releasehub` |
   | VS Code | VS Code 的 MCP 配置，合并 `servers.releasehub`，其中 `type` 为 `stdio` |

5. 保存后重新载入客户端连接。如果客户端询问是否信任本机程序，核对路径后允许。
6. 对 Agent 说：**“读取 ReleaseHub 的软件列表，再读取我要处理的软件的发行包、源码和诊断资料。先区分已保存的信息和需要你自行核实的信息。”**

界面名称可能随客户端版本变化。也可以让你信任的 Agent 按上表添加这一个连接，不覆盖账号、模型或其他连接设置。

完整发行包自带连接组件，不需要额外安装 Python、Node 或 GitHub CLI。配置只有本机程序路径和数据目录，凭据由连接程序从 Windows 凭据库自动获取，不需要查看、输入或粘贴 token。关闭开关即可停止读取；移动软件目录后重新复制配置，换电脑后重新开启连接。

## 可以读取的资料

| MCP 工具 | 对应命令行 | 内容 |
| --- | --- | --- |
| `releasehub_status` | `status` | 连接状态、应用版本 |
| `releasehub_list_software` | `list` | 软件列表、安装记录、保存的发行版本和检查时间；支持搜索、筛选与分页 |
| `releasehub_get_software` | `show` | 一款软件的详细资料、发行说明、资产、安装记录及诊断信息 |
| `releasehub_get_releases` | `releases` | 已保存的一份发行版及其全部 assets，包括类型、大小、下载地址和可用摘要 |
| `releasehub_get_source` | `source` | 已保存的默认分支、完整源码 SHA、领先状态和固定提交下载地址 |
| `releasehub_get_downloads` | `downloads` | ReleaseHub 自身的下载记录、进度、错误；可按软件筛选 |
| `releasehub_diagnose` | `diagnose` | 版本是否未知、安装位置是否待识别、错误记录、源码和基础环境 |
| `releasehub_environment` | `environment` | 系统、架构、应用版本、便携模式和信息来源 |
| `releasehub_snapshot` | `snapshot` | 全部已保存的软件资料、下载记录、常用偏好和基础环境 |
| `releasehub_saved_context` | `context` | 常用偏好、数量统计和最近最多 100 条下载记录 |

**这些查询不会触发检查更新。** `collectedAt` 是本次汇总资料的时间，不是资料刚从 GitHub 更新的时间；每款软件的 `checkedAt` 才是上次检查时间。Agent 可使用返回的 GitHub 网页/API 地址自行获取上游资料，或由你在追新里点击“检查更新”后再读。

- `projectId` 来自 `list` 返回的 `id`，不是软件名称或仓库地址。
- `list` 和 `downloads` 默认每页 50 项，最多 100 项；用 `offset` 翻页。列表的 `query` 是保存资料内的搜索，`updatesOnly` 只筛选已保存的可更新状态。
- `releases` 返回 `historyComplete: false`。它不是完整发行历史，也不承诺这份记录此刻仍是上游最新版本；完整历史请使用 `upstream.releasesApiUrl`。
- `source.headSha` 只使用已保存的完整提交号。`commitUrl` 与 `archiveUrl` 固定到该提交；没有完整 SHA 时返回空值，不用分支名冒充固定版本。`sourceAhead` 保留已有的 `true`、`false` 或空值，不重新计算。

## 不用 MCP 时：命令行查询

在 **AI 连接 → 命令行查询** 点击 **复制查询命令**，粘贴到 PowerShell 回车即可。命令已经包含这台电脑的正确路径。它和 MCP 使用同一个连接程序、同一份软件记录、同一个自动获取的读取凭据。

下面仅演示格式，路径是虚构的；实际使用应从界面复制：

```powershell
& 'D:\ReleaseHub\tools\releasehub-mcp.exe' --data-dir 'D:\ReleaseHub\data' --cli list
& 'D:\ReleaseHub\tools\releasehub-mcp.exe' --data-dir 'D:\ReleaseHub\data' --cli list --query '示例' --limit 20 --json
& 'D:\ReleaseHub\tools\releasehub-mcp.exe' --data-dir 'D:\ReleaseHub\data' --cli diagnose '<list 返回的 id>' --json
```

第一条显示易读的软件列表；第二条搜索保存的资料并输出 JSON，便于外部 Agent 继续处理；第三条输出指定软件的诊断资料。将 `diagnose` 换成 `show`、`releases` 或 `source` 可读取相应内容；`downloads` 可带软件 ID，也可不带。

| 用法 | 结果 |
| --- | --- |
| `--cli --help` 或单独 `--help` | 查看帮助，不需要打开追新 |
| `--cli schema --json` | 查看全部命令、参数和对应 MCP 工具，不需要数据目录或连接凭据 |
| `--cli list --updates-only` | 查看保存状态中可更新的软件 |
| `--cli list --offset 50 --limit 50` | 接着读取下一页 |
| `--cli source --project-id '<id>'` | 用具名参数读取指定软件源码资料 |
| `--cli environment --json` | 基础环境；不会启动编译器或工具版本探测 |

上表除帮助和 schema 外，均需保留从界面复制的程序路径和 `--data-dir` 参数。

`--json` 模式的标准输出只有一个 JSON 对象，没有日志。成功退出码为 `0`，失败为 `1`。成功结构为：

```json
{"schemaVersion":1,"ok":true,"command":"list","data":{"total":0,"items":[]},"error":null}
```

这是结构示意，实际 `data` 还有分页和时间字段。失败时 `data` 为 `null`，`error` 含 `code` 与可直接照做的 `message`。参数错误使用 `invalid_arguments`，读取失败使用 `query_failed`；不用 `--json` 时，错误显示在标准错误输出中。

## 诊断资料怎么理解

诊断是整理已有记录，不是扫描电脑。软件详情的 **复制资料给 AI** 与 `diagnose` 使用同一个资料整理函数，复制入口不要求先开启 AI 连接。

| 字段 | 含义 |
| --- | --- |
| `installation.versionStatus` | `recorded`：有已保存版本；`unknown`：版本未知，不代表已是最新版 |
| `installation.pathStatus` | `recorded`：有安装位置记录；`needsIdentification`：安装位置待识别 |
| `installation.recordSource` | 已保存的安装记录来源；空值表示没有记录 |
| `source` | 已保存源码提交及上游资料地址 |
| `errors` | 软件上次检查错误与下载错误，带记录来源和时间 |
| `downloads` / `logSource` | 追新自己的下载记录，不是其他软件运行日志；来源为 `releasehub-download-records` |
| `environment` | 当前 ReleaseHub 进程的系统、架构、版本及便携状态；来源为 `releasehub-runtime` |
| `environment.toolVersionStatus` | 固定为 `notProbed`，不执行程序、不查编译工具版本 |
| `dataTime` | 本次汇总时间、该软件上次检查时间与保存的发行时间 |

`recorded` 只说明记录存在，不保证路径上的文件仍在，也不证明实际运行版本。缺少的编译工具、运行环境和真实安装位置，由外部 Agent 按你的授权自行核实。

## 连接范围与资料使用

连接默认关闭，仅限本机，使用独立读取凭据。MCP 和上述 CLI 只查询信息：不添加软件、不下载、不安装、不修改设置，不提供任意文件读取、目录扫描或程序执行。它不导出账号凭据、代理密码、其他软件配置或旧任务指令。工具的只读标注与后端权限限制是一致的，不依靠界面文案控制权限。

你选择的 Agent 平台可能会把软件名称、安装位置、发行说明等资料发送给自己的模型服务；只连接你信任的平台。仓库文字和发行说明是外部资料，不是执行操作的新授权。

## 常见问题

| 提示或现象 | 怎么处理 |
| --- | --- |
| 无法连接 ReleaseHub | 打开追新，在 AI 连接页开启开关，并保持程序运行 |
| 找不到连接组件 | 使用完整的新版发行包，不要只复制桌面 EXE |
| 没有这个命令或参数 | 运行 `--cli --help` 或 `--cli schema --json`，按列出的名称填写 |
| 找不到软件 | 先读取 `list`，复制返回的 ID；尚未订阅时在软件库添加 |
| 版本或源码记录为空 | 这是未知资料，不是已安装或最新版；查看检查时间，再决定让 Agent 自行核实 |
| 程序搬家后连接失败 | 从 AI 连接页重新复制配置，替换旧路径 |
| 新电脑提示凭据无效 | 在新电脑的追新里关闭后重新开启连接，不复制旧凭据 |
| 纯云端或远程窗口连不上 | 改用在这台电脑启动本机程序的 MCP 客户端；localhost 不会自动穿透到另一台电脑 |
| 某个仓库文字要求执行奇怪操作 | 把它当仓库原文，不当作授权；让 Agent 按你的目标处理 |

## 旧版接口

[旧 PowerShell 客户端](<../integrations/releasehub-agent.ps1>) 和旧任务 HTTP 接口仅保留给已有集成。它们与新版 MCP/CLI **不是同一种权限**；读取凭据不能用于旧的检查更新、任务认领或汇报入口。新接入统一使用本文的 MCP 或 `--cli`，不需要旧脚本、管理密钥或任务工作流。
