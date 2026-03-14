import { defineConfig } from 'vitepress'

export default defineConfig({
  title: 'pikpaktui',
  description: 'A terminal-based client for PikPak cloud storage',
  base: '/pikpaktui/',

  head: [
    ['link', { rel: 'icon', type: 'image/svg+xml', href: '/pikpaktui/favicon.svg' }],
  ],

  locales: {
    root: {
      label: 'English',
      lang: 'en',
      themeConfig: {
        nav: [
          { text: 'Guide', link: '/guide/getting-started' },
          { text: 'CLI', link: '/cli/' },
          {
            text: 'Download',
            link: 'https://github.com/Bengerthelorf/pikpaktui/releases/latest',
          },
        ],
        sidebar: {
          '/guide/': [
            {
              text: 'Getting Started',
              items: [
                { text: 'Getting Started', link: '/guide/getting-started' },
              ],
            },
            {
              text: 'Usage',
              items: [
                { text: 'TUI Guide', link: '/guide/tui' },
                { text: 'Configuration', link: '/guide/configuration' },
                { text: 'Shell Completions', link: '/guide/shell-completions' },
              ],
            },
          ],
          '/cli/': [
            {
              text: 'CLI Reference',
              items: [
                { text: 'Overview', link: '/cli/' },
                { text: 'Command Reference', link: '/cli/commands' },
              ],
            },
          ],
        },
      },
    },
    zh: {
      label: '简体中文',
      lang: 'zh-Hans',
      themeConfig: {
        nav: [
          { text: '指南', link: '/zh/guide/getting-started' },
          { text: 'CLI', link: '/zh/cli/' },
          {
            text: '下载',
            link: 'https://github.com/Bengerthelorf/pikpaktui/releases/latest',
          },
        ],
        sidebar: {
          '/zh/guide/': [
            {
              text: '入门',
              items: [
                { text: '快速开始', link: '/zh/guide/getting-started' },
              ],
            },
            {
              text: '使用',
              items: [
                { text: 'TUI 指南', link: '/zh/guide/tui' },
                { text: '配置', link: '/zh/guide/configuration' },
                { text: 'Shell 补全', link: '/zh/guide/shell-completions' },
              ],
            },
          ],
          '/zh/cli/': [
            {
              text: 'CLI 参考',
              items: [
                { text: '概览', link: '/zh/cli/' },
                { text: '命令参考', link: '/zh/cli/commands' },
              ],
            },
          ],
        },
      },
    },
    'zh-Hant': {
      label: '正體中文',
      lang: 'zh-Hant',
      themeConfig: {
        nav: [
          { text: '指南', link: '/zh-Hant/guide/getting-started' },
          { text: 'CLI', link: '/zh-Hant/cli/' },
          {
            text: '下載',
            link: 'https://github.com/Bengerthelorf/pikpaktui/releases/latest',
          },
        ],
        sidebar: {
          '/zh-Hant/guide/': [
            {
              text: '入門',
              items: [
                { text: '快速開始', link: '/zh-Hant/guide/getting-started' },
              ],
            },
            {
              text: '使用',
              items: [
                { text: 'TUI 指南', link: '/zh-Hant/guide/tui' },
                { text: '設定', link: '/zh-Hant/guide/configuration' },
                { text: 'Shell 補全', link: '/zh-Hant/guide/shell-completions' },
              ],
            },
          ],
          '/zh-Hant/cli/': [
            {
              text: 'CLI 參考',
              items: [
                { text: '概覽', link: '/zh-Hant/cli/' },
                { text: '命令參考', link: '/zh-Hant/cli/commands' },
              ],
            },
          ],
        },
      },
    },
  },

  themeConfig: {
    logo: '/images/icon.svg',

    nav: [
      { text: 'Guide', link: '/guide/getting-started' },
      { text: 'CLI', link: '/cli/' },
      {
        text: 'Download',
        link: 'https://github.com/Bengerthelorf/pikpaktui/releases/latest',
      },
    ],

    sidebar: {
      '/guide/': [
        {
          text: 'Getting Started',
          items: [
            { text: 'Getting Started', link: '/guide/getting-started' },
          ],
        },
        {
          text: 'Usage',
          items: [
            { text: 'TUI Guide', link: '/guide/tui' },
            { text: 'Configuration', link: '/guide/configuration' },
            { text: 'Shell Completions', link: '/guide/shell-completions' },
          ],
        },
      ],
      '/cli/': [
        {
          text: 'CLI Reference',
          items: [
            { text: 'Overview', link: '/cli/' },
            { text: 'Command Reference', link: '/cli/commands' },
          ],
        },
      ],
    },

    socialLinks: [
      { icon: 'github', link: 'https://github.com/Bengerthelorf/pikpaktui' },
    ],

    editLink: {
      pattern: 'https://github.com/Bengerthelorf/pikpaktui/edit/main/docs/vitepress/:path',
    },

    footer: {
      message: 'Released under the Apache-2.0 License.',
      copyright: 'Copyright © 2024-present Bengerthelorf',
    },

    search: {
      provider: 'local',
    },
  },
})
