# ReleaseHub · 追新

**把分散的 GitHub 软件更新放在一起。** 面向 Windows 10/11 x64，中文界面、浅色/深色主题、便携数据，不要求注册账户，也不附带模型服务。

[下载发布版本](https://github.com/huai-bot/releasehub/releases) · [便携使用说明](docs/PORTABLE.txt) · [连接 Agent](docs/AGENT_GUIDE.md) · [安全说明](SECURITY.md)

![ReleaseHub 浅色界面，显示三个实际核对的软件](docs/images/main-window.png)

## 开始使用

1. 在发布页下载名称带 **windows-x64-portable.zip** 的文件。
2. 右键 → **全部解压**。把里面的 ReleaseHub 文件夹放到可写的位置，例如 `D:\TOOLS\ReleaseHub`，再打开文件夹中的应用程序。
3. 点击 **添加软件**，粘贴 GitHub 项目地址，选择正式版或包含预览版。
4. 点击 **检查更新**，确认推荐的 Windows 安装包，再下载。
5. 下载结束后点击安装/准备按钮。安装程序会在确认后打开；免安装版只准备到追新自己的目录，不覆盖你的原软件。

也提供名称带 **windows-x64-setup.exe** 的安装版。两种版本都使用系统共享的 Microsoft Edge WebView2；电脑缺少它时，安装版会提示补齐。便携版应先解压，不能直接在压缩包里运行。

> 发布文件未购买商业代码签名证书，Windows 可能显示“未知发布者”。请只从本仓库下载；发行页附有 SHA-256 校验值。

## 能做什么

| 日常操作 | 已提供的功能 |
| --- | --- |
| 集中追新 | 添加仓库、搜索、正式/预览频道、集中检查、发布说明、按软件记住下载偏好 |
| 选对文件 | 识别 Windows/x64，排除其他系统、ARM、源码和辅助组件；无法明确选择时由你确认 |
| 管理下载 | 批量加入队列、进度、取消、失败重试；流式写盘，不将整个安装包放进内存 |
| 检查完整性 | 计算 SHA-256；上游提供摘要时进行比对，只有本地摘要时不会标成“上游校验通过” |
| 安装和更新 | 确认后打开 EXE/MSI 安装程序；ZIP 或已适配的独立 EXE 放入独立受管目录 |
| 保留旧版本 | 列出已准备的免安装版本、打开对应目录、确认后切换当前记录；不删除旧版或混合两版配置 |
| 定时提醒 | 可关闭的定时检查、通知和自动下载；绝不在后台自动运行安装程序 |
| 移动数据 | 便携目录随文件夹移动；订阅导入/导出；搬到新电脑后重新配置本机授权 |
| GitHub 日常管理 | 网页授权登录、搜索和浏览仓库、Star/取消 Star、我的仓库、近期动态；确认后修改仓库说明和主页 |
| 连接自己的 AI | 给 Codex / Claude Code 提供已保存的软件、版本和下载记录；默认关闭、只读，不需要填写模型密钥 |

**下载完成不等于安装完成。** EXE/MSI 安装器运行后，请在软件详情中按实际情况记录版本。免安装更新使用独立版本目录；需要迁移软件个人配置时，先按上游说明处理，旧目录会保留。

### 首批实际核对的软件

- [FFmpegFreeUI](https://github.com/Lake1059/FFmpegFreeUI)：选择 x64 主程序，不把辅助更新器或 ARM64 程序当成目标。
- [ApkStart](https://github.com/RoutineMiku/apkstart)：区分轻量便携包、完整安装包和独立运行组件包。
- [Kazumi](https://github.com/Predidit/Kazumi)：优先 Windows ZIP，不混入 Android、Linux 或 macOS 文件。

实际检查过这三个仓库的发行记录，并完成 Kazumi Windows ZIP 的真实下载、上游摘要比对和独立目录准备；没有运行或修改用户原有软件。版本号始终从 GitHub 获取，不写死在程序里。[适配资料](docs/upstream-profiles.md)

## 便携和隐私

- 便携发行包包含模式标记，程序把数据库、下载、受管版本和网页组件缓存放在旁边的 `data/` 中。
- 移动之前从右下角托盘菜单选择 **退出**，再移动整个文件夹。重复打开同一份资料库会被拦下。
- 本机 AI 凭据保存在 Windows 凭据管理器。GitHub 由官方 CLI 授权，追新只接受系统凭据库中的有效登录；授权配置留在本机应用数据区，不进入便携导出或官方发行包。
- 在另一台电脑打开时需要重新授权；不要把你用过的完整数据文件夹直接作为“干净安装包”发给别人。
- 不带个人配置的分发请使用本仓库发布的原始便携 ZIP。

公开仓库无需登录即可查询。匿名额度不足时可等待恢复，或在 **GitHub** 页面完成网页授权；已连接账号也会用于添加软件和检查发行版本。程序不会借用开发者或其他 AI 工具的登录信息。

## GitHub 日常管理

1. 点击左侧 **GitHub → 登录 GitHub**，复制显示的短码。
2. 点击 **打开 GitHub 授权页**，在官网核对账号并授权，再回到追新。
3. 使用搜索、Stars、我的仓库和近期动态；点开仓库查看详情和 README。
4. 可 Star / 取消 Star、将公开软件加入软件库；有管理权限时，可确认修改仓库说明与主页。

登录使用随包附带的官方 **GitHub CLI 2.100.0**，不是追新自己的 OAuth 应用；授权包含私有仓库读写能力，与同机 CLI 可能共用授权。**仅断开追新**不会注销其他工具。无需另外安装命令行工具，也不需要手填令牌。

私有仓库可浏览和修改说明，暂不加入软件更新列表。Stars 不等于 Watch；动态可能延迟，不是完整实时消息。收藏/自有仓库的搜索只筛选当前页，页面有明确提示。没有创建/删除仓库、自动提交代码等入口。

## 连接自己的 AI

这是一个**默认关闭、只读的本机连接**：

1. 点击左侧 **AI 连接**，开启 **允许本机 AI 读取信息**。
2. 选择 Claude Code 或 Codex，复制无密钥配置，加入你的客户端并保留原有设置。
3. 重新打开 AI 会话，保持追新运行，然后让它读取软件列表、版本和下载记录。

不在追新里填写模型密钥，也不由追新执行 AI 操作。此连接不能检查更新、下载、安装、改数据或操作 GitHub 账号；读取的是上次保存的结果。安装位置等资料会交给你选择的 AI 客户端，请只连接可信客户端。详见 [AI 连接步骤](docs/AGENT_GUIDE.md)。

旧任务/PowerShell 接口仅为已配置的集成保留，使用不同凭据，不再作为正常界面流程。新版只读凭据不能使用旧接口的写操作。

## 开发

Tauri 2 + Svelte 5 + TypeScript + Rust + SQLite。当前构建使用 Node.js 26.5.0、pnpm 11.17.0、Rust 1.97.0 和 Windows C++ 构建工具。

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm test
pnpm build
.\scripts\prepare-github-cli.ps1
cargo test --locked --manifest-path src-tauri/Cargo.toml --lib
cargo build --locked --manifest-path src-tauri/Cargo.toml --bin releasehub-mcp
pnpm desktop
```

浏览器开发预览会明确标出“界面预览”，不会执行检查、下载、安装或保存凭据。正式桌面程序不会在服务失败时偷换成演示数据。

[架构和接口契约](docs/ARCHITECTURE.md) 包含发行追踪、下载、便携安装、GitHub 日常管理和只读 AI 连接。GitHub CLI 的版本与官方发行摘要固定在[组件准备脚本](scripts/prepare-github-cli.ps1)，其 MIT 许可随包附带。

[Windows 构建流程](.github/workflows/build-windows.yml) 通过 GitHub Actions 的 **Run workflow** 手动启动，完成检查、测试、EXE/NSIS 编译和干净便携包生成。使用锁定依赖、最小仓库权限，不复制开发者数据或凭据。打包规则见 [打包脚本](scripts/package-windows.ps1)。

## 许可

[MIT](LICENSE) · huai-bot
