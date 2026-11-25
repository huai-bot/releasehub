import type { AgentIntegration, GithubActivityPage, GithubListRepositoryPage, GithubRepository, GithubRepositoryDetail, GithubRepositoryPage, GithubRepositoryTab, GithubStatus, GithubUserList, RepositoryNote } from './types';
import { filterGithubPage, GITHUB_DEVICE_URL } from './github';

export type GithubPreviewScenario = 'connected' | 'anonymous' | 'waiting' | 'unavailable' | 'empty' | 'error';
const exampleDate = '2026-11-14T08:30:00Z';

/** Synthetic identities and values. Only the explicit development preview imports this file. */
export function createPreviewGithubStatus(scenario: GithubPreviewScenario): GithubStatus {
  const connected = ['connected', 'empty', 'error'].includes(scenario);
  return {
    available: scenario !== 'unavailable', connected, provider: 'github-cli', error: null,
    account: connected ? { login: 'releasehub-demo', name: '追新演示账号', avatarUrl: null, url: 'https://github.com/releasehub-demo' } : null,
    login: scenario === 'waiting' ? { state: 'waiting', userCode: 'DEMO-1234', verificationUri: GITHUB_DEVICE_URL, message: '这是示例短码，不能用于登录。' } : null,
  };
}

function repository(id: number, fullName: string, description: string, extra: Partial<GithubRepository> = {}): GithubRepository {
  return {
    id, name: fullName.split('/')[1], owner: fullName.split('/')[0], fullName, description,
    url: `https://github.com/${fullName}`, private: false, archived: false, fork: false,
    language: 'TypeScript', stars: 128, forks: 12, openIssues: 5, hasIssues: true, hasDiscussions: true, updatedAt: exampleDate,
    defaultBranch: 'main', homepage: null, topics: ['windows', 'desktop'], canAdmin: false, starred: null, ...extra,
  };
}
function examples(): GithubRepository[] {
  return [
    repository(901, 'Lake1059/FFmpegFreeUI', '界面示例：直观的音视频处理工具；此处数量与状态均非实时数据。', { language: 'Visual Basic .NET', stars: 2450, starred: true }),
    repository(902, 'releasehub-demo/window-notes', '示例仓库：把窗口中的灵感和待办收进一份简洁清单。', { stars: 326, canAdmin: true, starred: false }),
    repository(903, 'releasehub-demo/release-desk', '示例仓库：汇集桌面工具的发行记录，按自己的节奏追踪。', { language: 'Rust', stars: 91, canAdmin: true, starred: true }),
    repository(904, 'releasehub-demo/private-plans', '示例私有仓库：汇集团队的工具规划与待办。', { private: true, stars: 0, canAdmin: true, starred: false, topics: [] }),
    repository(905, 'example-tools/archive-browser', '示例仓库：快速浏览本地归档内容，不改动原文件。', { language: 'C#', stars: 670, starred: null }),
  ];
}
export function createPreviewGithubRepositories(tab: GithubRepositoryTab, query: string, page: number, scenario: GithubPreviewScenario): GithubRepositoryPage {
  if (scenario === 'error') throw new Error('示例：GitHub 暂时无法连接，请稍后重试。');
  if (scenario === 'empty') return { items: [], page, hasMore: false, totalCount: tab === 'explore' ? 0 : null };
  const items = examples().filter((item) => tab === 'owned' ? item.canAdmin : tab === 'starred' ? item.starred === true : !item.private);
  const matches = tab === 'explore' ? filterGithubPage(items, query) : items;
  const start = (page - 1) * 3;
  const connected = createPreviewGithubStatus(scenario).connected;
  return { items: matches.slice(start, start + 3).map((item) => ({ ...item, canAdmin: connected && item.canAdmin, starred: connected ? item.starred : null })), page, hasMore: matches.length > start + 3, totalCount: tab === 'explore' ? matches.length : null };
}
export function createPreviewGithubDetail(name: string, connected: boolean): GithubRepositoryDetail {
  const item = examples().find((entry) => entry.fullName === name) ?? repository(999, name, '界面预览示例仓库。');
  return {
    repository: { ...item, canAdmin: connected && item.canAdmin, starred: connected ? item.starred ?? false : null },
    readme: `# ${item.name}\n\n> 界面预览示例\n\n一个简洁的 Windows 桌面工具。在追新中，先阅读项目说明，再选择要持续关注的软件。\n\n## 使用方式\n\n- 搜索仓库或浏览 Stars\n- 通过分类整理，给仓库写下好记的本地备注\n- 加入软件库，订阅后续版本更新\n\n[查看 GitHub 原页](${item.url})`,
  };
}
export function createPreviewGithubActivity(page: number, scenario: GithubPreviewScenario): GithubActivityPage {
  if (scenario === 'error') throw new Error('示例：近期活动暂时读取失败，请重试。');
  if (scenario === 'empty' || page > 1) return { items: [], page, hasMore: false };
  return {
    items: [
      { id: 'demo-event-1', actor: 'example-developer', avatarUrl: null, repository: 'releasehub-demo/window-notes', type: 'ReleaseEvent', summary: '发布了一个发行版（示例）', url: 'https://github.com/releasehub-demo/window-notes/releases', createdAt: exampleDate },
      { id: 'demo-event-2', actor: 'example-maintainer', avatarUrl: null, repository: 'releasehub-demo/release-desk', type: 'PushEvent', summary: '更新了仓库内容（示例）', url: 'https://github.com/releasehub-demo/release-desk', createdAt: exampleDate },
    ], page, hasMore: false,
  };
}
export function createPreviewGithubLists(scenario: GithubPreviewScenario): GithubUserList[] {
  if (scenario === 'error') throw new Error('示例：分类暂时读取失败，请重试。');
  if (scenario === 'empty') return [];
  return [
    { id: 'demo-tools', name: '桌面工具', description: '常用的 Windows 软件', url: 'https://github.com/stars/releasehub-demo/lists/desktop-tools', itemCount: 2 },
    { id: 'demo-media', name: '音视频', description: '剪辑、转码与素材管理', url: 'https://github.com/stars/releasehub-demo/lists/media', itemCount: 1 },
  ];
}

export function createPreviewGithubListRepositories(listId: string, cursor: string | undefined, scenario: GithubPreviewScenario): GithubListRepositoryPage {
  if (scenario === 'error') throw new Error('示例：Stars 暂时读取失败，请重试。');
  let items = scenario === 'empty' ? [] : examples().filter((item) => item.starred === true || item.id === 905).map((item) => ({ ...item, starred: true }));
  if (listId === 'demo-tools') items = items.filter((item) => item.id !== 905);
  if (listId === 'demo-media') items = items.filter((item) => item.id === 901);
  if (listId === 'unlisted') items = items.filter((item) => item.id === 905);
  const start = Number(cursor ?? 0);
  const hasMore = items.length > start + 2;
  return { items: items.slice(start, start + 2), totalCount: items.length, hasMore, endCursor: hasMore ? String(start + 2) : null };
}

export function createPreviewRepositoryNotes(): RepositoryNote[] {
  return [{ repository: 'Lake1059/FFmpegFreeUI', name: '视频工具箱', description: '日常转码、压缩与音轨处理', updatedAt: exampleDate }];
}

export function createPreviewRepositoryLists(repository: string): string[] {
  return repository === 'Lake1059/FFmpegFreeUI' ? ['demo-tools', 'demo-media'] : repository === 'releasehub-demo/release-desk' ? ['demo-tools'] : [];
}

export function createPreviewAgentIntegration(): AgentIntegration {
  const command = 'C:\\Example\\ReleaseHub\\tools\\releasehub-mcp.exe';
  const args = ['--data-dir', 'C:\\Example\\ReleaseHub\\data'];
  return {
    command, args, readOnly: true,
    claudeConfig: JSON.stringify({ mcpServers: { releasehub: { command, args } } }, null, 2),
    codexConfig: `[mcp_servers.releasehub]\ncommand = ${JSON.stringify(command)}\nargs = ${JSON.stringify(args)}`,
    genericConfig: JSON.stringify({ command, args }, null, 2),
    cursorConfig: JSON.stringify({ mcpServers: { releasehub: { command, args } } }, null, 2),
    vscodeConfig: JSON.stringify({ servers: { releasehub: { type: 'stdio', command, args } } }, null, 2),
    cliCommand: `& '${command}' --data-dir '${args[1]}' --cli`,
    tools: [{ name: 'repository_notes', description: '查看仓库的本地备注' }],
  };
}
