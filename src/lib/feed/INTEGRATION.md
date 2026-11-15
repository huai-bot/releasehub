# 官网首页动态接入

本模块读取独立 GitHub 官网窗口实际加载的页面文字，不调用 Events、RSS 或假定的内部接口。默认来源是 `https://github.com/dashboard-feed`；这是 GitHub 官方仍提供的动态页，不承诺与新首页的所有模块相同。

## Host 接入

在主程序中引入 `github_feed`，复用本次已建立的 `Arc<ProxyState>`：

```rust
let feed = GitHubFeedService::new(machine_local_root, proxy.clone());
app.manage(feed.clone()); // 必须单独 manage 这个 Arc，供 receiver 取用
// HostState 同时持有 feed.clone()
```

- `machine_local_root: PathBuf` 必须是本机账户目录中的绝对路径，不能是便携 `data`、浏览器数据或导出目录。应按应用实例隔离，例如主程序现有的 `app_local_data_dir()/accounts/<instance-id>`。
- `GitHubFeedService::handle(self: &Arc<Self>, app: &AppHandle, action: &str, payload: Value) -> Result<Value, String>`：将 `feed_*` 在 `github_*` 之前分流；支持 `feed_status/connect/refresh/more/disconnect`，参数都为 `{}`。
- `generate_handler!` 同时登记 `dispatch` 与 `github_feed::feed_receive`。
- 主窗口权限增加 `allow-host-dispatch`。独立 `github-feed` capability 只允许 `allow-feed-receive`，限定 `github-feed` 标签、远端 `https://github.com/*`，`local:false`。不要给它 `dispatch`、`core:default`、shell、opener 或文件权限。主 `dispatch` 仍应检查调用窗口标签为 `main`。
- 保存代理设置或密码后调用 `feed.restart_for_proxy_change(app).await`。WebView2 代理是启动设置，修改共享 HTTP 客户端并不会改变已经打开的网页窗口。
- 退出整个应用无需额外 shutdown。专属网页会话留在本机，下次连接可继续；只有 `feed_disconnect` 清除此模块的网页会话，不影响 gh CLI 或用户浏览器。

返回 `GithubFeedPage`，字段与共享 types 一致；`requestId` 和递增 `sessionRevision` 用于拒绝过期结果。后端限制 160 条、2 MiB 单份消息，文本字段和 URL 再次检查；分页只点击已加载 DOM 中识别到的“更多”控件。

## 组件

[GithubFeed.svelte](<../../components/GithubFeed.svelte>) 的参数：

```ts
{ preview: boolean;
  onopen: (url: string) => void;
  onadd: (repository: GithubRepository) => Promise<boolean>;
  onnotice: (message: string, tone?: Tone) => void; }
```

组件应放在 gh 授权门槛之外；网页登录与设备授权是独立会话。加入软件库前，组件自己调用 `github_repository` 取得仓库详情，再交给 `onadd`。所有卡片以文字显示，不使用 `{@html}`。预览只在开发环境加载明确标注的合成示例，真实失败不回退示例。

## 代理与本机资料

- 系统模式：使用 Windows WebView2 的系统代理/PAC；不读取其他浏览器资料。
- 直连模式：启动参数 `--no-proxy-server`。
- 手动模式：Tauri `proxy_url` 支持 HTTP/SOCKS5，且不与 `additional_browser_args` 混用（后者会覆盖 Wry 自动生成的代理参数）。带代理账号密码时明确提示改用系统代理或本机无密码代理，不把密码放进命令参数。
- 本机目录为 `<machine_local_root>/github-feed-webview/profile-<UUID>`，`current.json` 仅记录专属目录标识和代理设置摘要。登出先更新目录指向，再清浏览数据、关闭窗口、清理旧专属目录；清理在操作锁内串行执行，不会与新连接抢用目录。文件仍被网页进程占用时会提示关闭应用后重试。
- 不扫描 Chrome/Edge 目录，不导入 cookie/凭据，不读取表单输入值。官网窗口禁止新窗口和下载，只允许导航至 GitHub HTTPS 官方域名。

## 已跑过的局部检查

使用已安装工具，不新增依赖，不启动服务器：

```text
node node_modules/vitest/vitest.mjs run src/lib/feed/dom-extractor.test.ts src/lib/feed/webview-receiver.test.ts
# 16/16 通过
node node_modules/vitest/vitest.mjs run --config src/lib/feed/ui.config.ts
# 7/7 通过；ui.spec.ts 单独收集，避免默认 SSR 测试入口误收集
cargo test --manifest-path src-tauri/Cargo.toml --lib github_feed::
# 由主代理集中执行；本模块含 URL、字段清理、顺序与上限、换号清空、专属目录清理测试
```

`impeccable detect src/components/GithubFeed.svelte --json` 返回空结果。合成网页测试覆盖现代 article、旧 news 形状、原始顺序、真实时间与原链接、危险链接/表单/隐藏内容排除、160 条边界、分页控件、空状态与结构变化；窗口脚本测试覆盖限定接收指令、来源与子框架、请求标识、防抖、持续变化、分页超时、换号旧卡片、历史导航恢复。组件测试覆盖独立连接、纯文本、仓库加入、退出清空、旧结果拒绝、错误重试、分页与预览隔离。

## 仍须真实桌面登录检查

本次没有读取真实账号的登录资料，也没有冒充已完成 GitHub 登录验证。页面结构由公开资料与合成片段建模；如果 GitHub 改版，模块会显示明确读取错误，不伪装成空列表或 Events 动态。

1. 桌面应用打开「GitHub → 首页动态 → 连接官网」，在独立官方窗口自行输入账号，完成密码/二次验证。确认动态自动回到主界面、窗口自动隐藏。
2. 点「官网窗口」与主界面对照前几条的顺序、文字、原链接及账号。默认应标注 Dashboard feed，不能写成经过验证的完整首页。
3. 点「加载更多」，确认后续内容接在后面；点「刷新」，确认回到官网新加载的顺序。切走再返回网页历史记录，确认仍会更新。
4. 在官网窗口切换账号，确认主界面旧内容清空后再显示新账号；退出网页后重新连接应需要重新登录，而 Chrome/Edge 与 gh 授权保持不变。
5. 改代理后重新连接，确认窗口确实重建；无网络/官网结构变化时看到错误而不是示例。分别检查 960×640 的浅色、深色、长内容、空列表和错误状态。窗口截图与整机登录检查由主代理集中进行。

参考：GitHub 官方 [Dashboard feed 更新说明](<https://github.blog/changelog/2025-09-04-the-dashboard-feed-page-gets-a-refreshed-faster-experience/>)、[个人首页说明](<https://docs.github.com/en/account-and-profile/get-started/personal-dashboard-quickstart>)。Tauri 2.12.5 / Wry 0.57.2 的本地源码已核对 `data_directory`、`initialization_script`、`on_navigation`、`on_page_load`、`proxy_url`、`clear_all_browsing_data` 和远端权限匹配。
