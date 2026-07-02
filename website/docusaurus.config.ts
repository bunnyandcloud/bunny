import {themes as prismThemes} from 'prism-react-renderer';
import type {Config} from '@docusaurus/types';
import type * as Preset from '@docusaurus/preset-classic';

const config: Config = {
  title: 'bunny',
  tagline: 'Coding in multiplayer mode with AI agents',

  favicon: 'img/logo.png',

  url: 'https://docs.bunnyandcloud.com',
  baseUrl: '/',
  trailingSlash: false,

  organizationName: 'bunnyandcloud',
  projectName: 'bunny',

  customFields: {
    docVersion: 'v0.5',
  },

  onBrokenLinks: 'warn',
  onBrokenMarkdownLinks: 'warn',

  markdown: {
    mermaid: true,
  },

  themes: [
    '@docusaurus/theme-mermaid',
    [
      require.resolve('@easyops-cn/docusaurus-search-local'),
      {
        hashed: true,
        language: ['en'],
        docsRouteBasePath: '/',
        indexBlog: false,
        highlightSearchTermsOnTargetPage: true,
        searchBarPosition: 'left',
      },
    ],
  ],

  i18n: {
    defaultLocale: 'en',
    locales: ['en'],
  },

  presets: [
    [
      'classic',
      {
        docs: {
          sidebarPath: './sidebars.ts',
          routeBasePath: '/',
          editUrl: 'https://github.com/bunnyandcloud/bunny/tree/main/website/',
        },
        blog: false,
        theme: {
          customCss: './src/css/custom.css',
        },
      } satisfies Preset.Options,
    ],
  ],

  themeConfig: {
    image: 'img/logo.png',
    colorMode: {
      defaultMode: 'dark',
      respectPrefersColorScheme: false,
    },
    navbar: {
      title: 'bunny',
      logo: {
        alt: 'bunny',
        src: 'img/logo.png',
        href: '/home',
        width: 56,
        height: 50,
      },
      items: [
        {
          type: 'search',
          position: 'left',
        },
        {
          href: 'https://bunnyandcloud.com',
          label: 'Website',
          position: 'right',
        },
        {
          href: 'https://github.com/bunnyandcloud/bunny',
          label: 'GitHub',
          position: 'right',
        },
      ],
    },
    footer: {
      style: 'dark',
      links: [
        {
          title: 'Getting started',
          items: [
            {label: 'Configure the server', to: '/getting-started/configure-server'},
            {label: 'Choose your path', to: '/getting-started/choose-your-path'},
            {label: 'Install with Docker', to: '/getting-started/install-docker'},
            {label: 'Install on Linux', to: '/getting-started/install-linux'},
            {label: 'First run', to: '/getting-started/first-run'},
          ],
        },
        {
          title: 'Team chats',
          items: [
            {label: 'Discord setup', to: '/team-chats/discord/setup'},
            {label: 'Discord workflows', to: '/team-chats/discord/workflows'},
            {label: 'Discord commands', to: '/team-chats/discord/commands'},
          ],
        },
        {
          title: 'Reference',
          items: [
            {label: 'Security', to: '/security/'},
            {label: 'CLI reference', to: '/reference/cli'},
            {label: 'API', to: '/api/'},
          ],
        },
        {
          title: 'More',
          items: [
            {label: 'bunnyandcloud.com', href: 'https://bunnyandcloud.com'},
            {label: 'GitHub', href: 'https://github.com/bunnyandcloud/bunny'},
          ],
        },
      ],
      copyright: `Open source · Self-hosted · bunnyandcloud.com`,
    },
    prism: {
      theme: prismThemes.github,
      darkTheme: prismThemes.vsDark,
    },
    mermaid: {
      theme: {light: 'neutral', dark: 'dark'},
    },
  } satisfies Preset.ThemeConfig,
};

export default config;
