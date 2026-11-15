# ReleaseHub · 追新

**把分散的 GitHub 软件更新放在一起。** 面向 Windows 10/11 x64，集中管理发行版、Stars 分类、项目备注和首页动态，通过 MCP 把软件资料交给你使用的 AI 平台。

[下载发布版本](<https://github.com/huai-bot/releasehub/releases>) · [便携使用说明](<docs/PORTABLE.txt>) · [连接 Agent](<docs/AGENT_GUIDE.md>) · [安全说明](<SECURITY.md>)

## 开始使用

1. 下载名称带 **windows-x64-portable.zip** 的文件，右键 → **全部解压**。
2. 把 ReleaseHub 文件夹放到可写的位置，例如 `D:\TOOLS\ReleaseHub`，双击其中的应用程序。
3. 点击 **添加软件**，粘贴 GitHub 项目地址，再点击 **检查更新**。
4. 核对 Windows 安装包并下载。下载结束后点击安装/准备按钮，按提示完成安装或打开程序目录。
5. 在 **设置 → 启动与导航** 中选择默认页面；拖动左侧页面可调整顺序。

也提供 **windows-x64-setup.exe** 安装版。两种版本都使用 Microsoft Edge WebView2；缺少运行环境时，安装版会提示补齐。

> 发布文件未购买商业代码签名证书，Windows 可能显示“未知发布者”。发行页附有 SHA-256 校验值，请核对下载来源。

## 能做什么

| 日常操作 | 功能 |
| --- | --- |
| 集中追新 | 订阅仓库、正式/预览频道、集中检查、发布说明、按软件记住下载偏好 |
| 选择与下载 | Windows/x64 文件识别、队列、进度、取消、失败重试、SHA-256 比对 |
| 安装与版本 | 打开 EXE/MSI 安装向导，ZIP 和适配的独立 EXE 保存到受管目录；切换已准备版本 |
| 网络代理 | 跟随系统、手动 HTTP/SOCKS5、直连，支持代理账号密码及连接测试 |
| GitHub 管理 | 搜索、我的仓库、Star/取消 Star、Lists 分类同步、批量添加分类 |
| 项目备注 | 为任意仓库记录本地名称和短说明，配合搜索和资料导入导出 |
| 首页动态 | 登录 GitHub 官网，按原页面顺序显示 Dashboard feed；同时保留关注动态 |
| 个人偏好 | 一体化浅色/深色标题栏、默认启动页、GitHub 默认栏目、侧栏拖动与键盘排序 |
| AI 资料连接 | 通用 MCP、Claude Code、Codex、Cursor、VS Code 配置，10 个信息工具和配套命令行 |
| 数据迁移 | 便携资料库，订阅、备注与偏好导入导出 |

EXE/MSI 安装向导完成后，可在软件详情中记录本机版本和位置。免安装软件按版本分别保存，各版本使用自己的目录与配置。

### 网络代理

打开 **设置 → 网络代理**：

- **跟随系统**：GitHub 请求和下载读取 Windows 手动代理或当前启动环境的代理地址。
- **手动设置**：填写如 `http://127.0.0.1:7890` 或 `socks5://127.0.0.1:1080`，需要认证时再填账号和密码。
- **不使用代理**：直接连接网络。

先点 **测试连接**，再点 **保存代理设置**。连接正常时显示绿色结果；GitHub 未登录查询额度用完时，另用黄色提示额度和预计恢复时间，不再把它误报为代理故障。账号密码错误或访问被拒绝仍会明确报错。

新设置用于后续请求；进行中的下载可取消后重新下载。本机 AI 连接始终直连。官网动态使用独立 WebView2，会在代理变更后重建连接；带账号密码的手动代理请改用系统代理或无密码的本机转发端口。

## GitHub 管理与首页动态

1. 在 **GitHub → 登录 GitHub** 复制短码，打开 GitHub 授权页并确认账号。
2. 进入 **Stars**，读取官网 Lists 分类；可新建、改名、删除分类，也可多选项目加入分类。
3. 项目可同时属于多个分类。“批量加入分类”保留原有分类；单个项目的“设置分类”以勾选结果为准。
4. 点开仓库 → **编辑本地备注**，填写便于记忆的项目名称和短说明。备注保存在追新；官网名称保持原样。
5. 进入 **首页动态 → 连接官网**，在独立官方窗口登录一次。完成后，页面内容自动显示在追新，可刷新、加载更多和查看原文。

切换到软件内其他页面再回到 **GitHub**，会保留已经加载的内容、当前分类、筛选和滚动位置，不重复加载。需要同步官网新内容时，点击列表或动态的 **刷新**；退出或更换账号会清掉旧账号内容。缓存仅在本次打开软件期间保留。

日常管理由随包附带的官方 **GitHub CLI 2.100.0** 授权。Lists 使用 GitHub 官方 GraphQL；若现有授权缺少分类管理权限，按页面提示补充授权。官网动态使用单独的网页登录，与 CLI 登录分别管理。

**关注动态**来自 GitHub 的 received events；**首页动态**来自实际加载的 `github.com/dashboard-feed` 页面。网页读取适配随 GitHub 页面结构更新。私有仓库可浏览和修改说明，软件更新列表目前面向公开仓库。

## 连接自己的 AI

1. 点击左侧 **AI 连接**，开启 **开启软件资料连接**。
2. 选择平台，复制配置到客户端的 MCP 设置。其他平台选择 **通用 JSON**。
3. 保持追新运行，重新打开 AI 会话，让它查询软件、版本、源码、安装位置和下载错误。
4. 需要单独处理某款软件时，在软件详情点击 **复制资料给 AI**，再把内容粘贴给 Agent。

**追新提供软件资料，外部 Agent 平台执行检查、编译、安装和修复。** 资料包含保存时间，以及版本未知、安装位置待识别等状态，方便 Agent 决定下一步。

随包的 `tools/releasehub-mcp.exe` 同时提供 `--cli` 查询方式，支持 `status`、`list`、`show`、`releases`、`source`、`downloads`、`diagnose`、`environment`、`snapshot` 和 `schema`，可输出 JSON。配置与完整用法见 [AI 连接指南](<docs/AGENT_GUIDE.md>)。

## 便携与迁移

- 便携包中的 `portable.flag` 使数据库、下载、受管版本和主界面缓存保存在旁边的 `data/` 中。
- 移动前从托盘菜单选择 **退出**，再复制整个文件夹。只分享订阅与备注时使用 **设置 → 导出订阅与备注**。
- GitHub 授权、代理密码和 AI 连接凭据保存在本机；官网动态会话也留在本机账户目录。换电脑后重新连接账号和代理。
- 分发给他人使用发布页中的原始 ZIP。

### 已适配的软件示例

- [FFmpegFreeUI](<https://github.com/Lake1059/FFmpegFreeUI>)：区分 x64 主程序、辅助更新器和 ARM64 程序。
- [ApkStart](<https://github.com/RoutineMiku/apkstart>)：区分轻量便携包、完整安装包和运行组件。
- [Kazumi](<https://github.com/Predidit/Kazumi>)：优先 Windows ZIP。

[上游适配资料](<docs/upstream-profiles.md>)

## 开发

Tauri 2 + Svelte 5 + TypeScript + Rust + SQLite。构建使用 Node.js 26.5.0、pnpm 11.17.0、Rust 1.97.0 和 Windows C++ 构建工具。

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm test
pnpm exec vitest run --config tests/github-ui.config.ts
pnpm exec vitest run --config tests/github-lists.config.ts
pnpm exec vitest run --config tests/preferences-ui.config.ts
pnpm exec vitest run --config src/lib/feed/ui.config.ts
pnpm build
.\scripts\prepare-github-cli.ps1
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
cargo build --locked --manifest-path src-tauri/Cargo.toml --bin releasehub-mcp
pnpm desktop
```

浏览器开发预览使用明确标注的示例数据。[架构和接口](<docs/ARCHITECTURE.md>) 描述各模块的连接方式；[官网动态接入说明](<src/lib/feed/INTEGRATION.md>) 包含网页登录检查步骤。

[Windows 构建](<.github/workflows/build-windows.yml>) 通过 GitHub Actions 的 **Run workflow** 手动启动，生成安装版、便携版和校验文件。打包内容使用显式清单，见 [打包脚本](<scripts/package-windows.ps1>)。

## 许可

[MIT](<LICENSE>) · huai-bot
