import { defineConfig } from "vitepress";

const REPO = "https://github.com/IcyBlue17/hoshiro";

// 部署到 GitHub Pages 子路径时：SITE_BASE=/hoshiro/ bun run build
const base = process.env.SITE_BASE ?? "/";

export default defineConfig({
  base,
  lang: "zh-CN",
  title: "Hoshiro",
  titleTemplate: ":title · Hoshiro",
  description: "基于 Tauri 2 的开源 macOS / Windows 语音输入法。按住快捷键说话，松开自动将润色后的文本插入光标处。",
  cleanUrls: true,
  lastUpdated: true,
  head: [
    ["link", { rel: "icon", type: "image/png", href: `${base}logo.png` }],
    ["meta", { name: "theme-color", content: "#151515" }],
    ["meta", { property: "og:title", content: "Hoshiro — macOS / Windows 语音输入法" }],
    ["meta", { property: "og:description", content: "基于 Tauri 2 的开源 macOS / Windows 语音输入法。直连模型服务商，数据完全本地存储。" }],
    ["meta", { property: "og:image", content: `${base}images/home.png` }],
  ],
  themeConfig: {
    logo: "/logo.png",
    nav: [
      { text: "首页", link: "/" },
      { text: "使用教程", link: "/guide/getting-started", activeMatch: "/guide/" },
      { text: "下载", link: `${REPO}/releases/latest` },
    ],
    sidebar: {
      "/guide/": [
        {
          text: "入门",
          items: [
            { text: "安装与首次设置", link: "/guide/getting-started" },
            { text: "从 Typeless 迁移", link: "/guide/migrate" },
          ],
        },
        {
          text: "使用",
          items: [
            { text: "输入模式与快捷键", link: "/guide/modes" },
            { text: "服务商配置", link: "/guide/providers" },
            { text: "个性化与词典", link: "/guide/personalization" },
            { text: "使用统计", link: "/guide/statistics" },
          ],
        },
        {
          text: "更多",
          items: [
            { text: "数据与隐私", link: "/guide/privacy" },
            { text: "常见问题", link: "/guide/faq" },
          ],
        },
      ],
    },
    socialLinks: [{ icon: "github", link: REPO }],
    editLink: { pattern: `${REPO}/edit/main/frontend/:path`, text: "在 GitHub 上编辑此页" },
    outline: { level: [2, 3], label: "本页目录" },
    docFooter: { prev: "上一页", next: "下一页" },
    lastUpdated: { text: "最后更新" },
    returnToTopLabel: "回到顶部",
    sidebarMenuLabel: "菜单",
    darkModeSwitchLabel: "外观",
    lightModeSwitchTitle: "切换到浅色模式",
    darkModeSwitchTitle: "切换到深色模式",
    search: {
      provider: "local",
      options: {
        translations: {
          button: { buttonText: "搜索文档", buttonAriaLabel: "搜索文档" },
          modal: {
            noResultsText: "没有找到相关结果",
            resetButtonTitle: "清除查询",
            footer: { selectText: "选择", navigateText: "切换", closeText: "关闭" },
          },
        },
      },
    },
    footer: {
      message: "基于 MIT 许可发布 · 与 Typeless 及各模型服务商无商业关联",
      copyright: "© 2026 Hoshiro",
    },
  },
});
