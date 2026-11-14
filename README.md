# ReleaseHub · 追新

一个轻量、便携优先的 Windows GitHub 软件更新中心。

> 项目正在实现中。发布的安装包和功能验证结果将随首个版本一起提供；当前源代码不代表已经完成所有功能。

## 目标

- 把分散的 GitHub 软件发行版放进一个清晰的中文界面。
- 正式版/预览版分别跟踪，记住下载偏好，集中检查、批量下载。
- 下载与安装状态分开；安装或替换需要确认。
- 便携数据跟随文件夹迁移，账户授权留在本机系统凭据管理器。
- 可选、本机、带鉴权的 Agent 编译任务接口，不内置模型密钥或私人 API。
- 给未来的个人 GitHub 管理功能留下扩展位置，不提前堆积功能。

## 开发

技术：Tauri 2、Svelte 5、TypeScript、Rust、SQLite。

需要 Node.js 22.12+、pnpm 11.17、Rust 和 Windows C++ 构建工具。

```powershell
pnpm install --frozen-lockfile
pnpm check
pnpm test
pnpm desktop
```

开发协作契约见 [架构说明](docs/ARCHITECTURE.md)。本项目不会读取或导入开发者的 DSH 配置、模型密钥或私人接口地址。

## 许可

[MIT](LICENSE) · huai-bot
