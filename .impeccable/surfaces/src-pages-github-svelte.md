---
version: 1
slug: "src-pages-github-svelte"
primary_target: "src/pages/Github.svelte"
related_targets: ["src/components/GithubDetail.svelte","src/components/AiConnection.svelte","src/pages/Connections.svelte"]
---

# GitHub daily management and external-AI connection

Scope: Operate surfaces inside ReleaseHub's approved desktop utility. The user confirmed existing sidebar/list composition, daily GitHub operations, and read-only external AI. Minimum window 960×640; default 1280×840. No mobile product, no new visual identity.

## Direction contract

THESIS: Manage repositories next to the software library, rather than embedding the GitHub website or putting an AI chat inside ReleaseHub.

OWN-WORLD: Inherit DESIGN.md's Industrial Slate & Cobalt Utility. Existing system fonts, semantic colors, 6px controls, 8px panels and compact rows remain authoritative. Preserve the corrected light/dark theme previews.

STORY: Explicitly connect an account, inspect repositories and README, then choose Star, subscription, or a confirmed description/homepage change. Separately enable a local read-only connection for an already-owned AI client. Show permissions before either connection.

FIRST VIEWPORT: Account row, repository tabs and search, then a compact list. Above 1250px the selected detail sits alongside; smaller supported windows switch to detail with a visible return action. AI connection shows its off switch and read-only explanation before configuration.

FORM: seed: none (pinned). The user confirmed retaining the current sidebar and list structure. Login, empty, error, private and disconnected states use the same frame; development demonstrations are clearly labelled and cannot mutate real accounts.

FINISH: unreviewed and undocumented is unfinished; this build ends with the finish review, the verdict, DESIGN.md, and every shipping raster carrying its provenance

No shipping raster imagery added. Existing design documentation is preserved; verify the extension against it rather than rewrite the visual system.
