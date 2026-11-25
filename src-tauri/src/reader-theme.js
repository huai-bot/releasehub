function applyReaderTheme({ theme, css }) {
  if (window !== window.top || location.origin !== 'https://github.com') return;
  const sheet = new CSSStyleSheet();
  sheet.replaceSync(css);
  document.adoptedStyleSheets = [...document.adoptedStyleSheets, sheet];

  const system = matchMedia('(prefers-color-scheme: dark)');
  const apply = () => {
    document.documentElement.dataset.releasehubTheme = theme === 'system' ? (system.matches ? 'dark' : 'light') : theme;
  };
  if (document.documentElement) apply();
  else {
    const observer = new MutationObserver(() => {
      if (document.documentElement) { observer.disconnect(); apply(); }
    });
    observer.observe(document, { childList: true });
  }
  if (theme === 'system') system.addEventListener('change', apply);
}
