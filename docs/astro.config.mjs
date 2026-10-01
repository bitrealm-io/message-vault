import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightSidebarTopics from 'starlight-sidebar-topics';
import mermaid from 'astro-mermaid';
import { satteri } from '@astrojs/markdown-satteri';
import { wrapTables } from './src/lib/satteri-wrap-tables.mjs';

// Documentation type, the landing page's (src/DESIGN.md): Cal Sans for
// headings and Inter for body copy, plus a mono face for every typed token.
// See src/styles/custom.css.
const fontsHref =
  'https://fonts.googleapis.com/css2' +
  '?family=Cal+Sans' +
  '&family=Inter:ital,wght@0,400;0,500;0,600;1,400' +
  '&family=IBM+Plex+Mono:wght@400;500' +
  '&display=swap';

const limitedBadge = {
  text: 'Limited',
  variant: 'caution',
};

const userGuideItems = [
  { label: 'Home', slug: 'docs/user' },
  {
    label: 'Get started',
    items: [
      'docs/user/get-started/what-is-message-crate',
      'docs/user/get-started/why-you-provide-backups',
      'docs/user/get-started/try-message-crate',
      'docs/user/get-started/your-own-messages',
      'docs/user/get-started/install-the-desktop-app',
    ],
  },
  {
    label: 'Prepare a backup',
    items: [
      'docs/user/prepare-a-backup',
      'docs/user/prepare-a-backup/iphone-ipad',
      'docs/user/prepare-a-backup/iphone-whatsapp',
      'docs/user/prepare-a-backup/android-sms',
      'docs/user/prepare-a-backup/android-whatsapp',
    ],
  },
  'docs/user/import-from-a-backup',
  'docs/user/browse-your-messages',
  {
    label: 'How do I…',
    items: [
      'docs/user/how-to/search',
      'docs/user/how-to/contacts-and-labels',
      'docs/user/how-to/saved-searches',
      'docs/user/how-to/trash',
      'docs/user/how-to/settings',
      'docs/user/how-to/owner-home',
      'docs/user/how-to/export-your-messages',
      'docs/user/how-to/convert-formats',
      'docs/user/how-to/media-and-privacy',
      { slug: 'docs/user/how-to/rescue-imports', badge: limitedBadge },
      'docs/user/how-to/update',
      'docs/user/how-to/troubleshooting',
    ],
  },
  'docs/user/glossary',
];

const developerItems = [
  'docs/developer',
  'docs/developer/contributing',
  'docs/developer/release',
  'docs/developer/rustdoc-style',
  {
    label: 'Architecture',
    items: [
      'docs/developer/design',
      'docs/developer/message-transfer',
      'docs/developer/architecture/common-message',
    ],
  },
  'docs/developer/docker',
  'docs/developer/reference/api',
  {
    label: 'HTTP problem types',
    collapsed: true,
    items: [{ autogenerate: { directory: 'docs/developer/reference/errors' } }],
  },
  {
    label: 'HTTP API reference',
    link: '/docs/developer/rustdoc/http/',
    attrs: { target: '_self' },
  },
  {
    label: 'Rust crate docs',
    link: '/docs/developer/rustdoc/',
    attrs: { target: '_self' },
  },
  {
    label: 'Formats',
    items: [
      'docs/developer/formats',
      'docs/developer/formats/mail-archive',
      'docs/developer/formats/sms-backup-restore-xml',
      'docs/developer/formats/convert',
      {
        label: 'SMS Backup & Restore',
        items: [
          'docs/developer/formats/sms-backup-restore/input',
          'docs/developer/formats/sms-backup-restore/mapping',
        ],
      },
      {
        label: 'SMS Backup+',
        items: [
          'docs/developer/formats/sms-backup-plus/format',
          'docs/developer/formats/sms-backup-plus/mapping',
        ],
      },
      {
        label: 'GO SMS Pro',
        items: ['docs/developer/formats/go-sms-pro/mapping'],
      },
      {
        label: 'iMazing',
        items: [
          'docs/developer/formats/imazing/input',
          'docs/developer/formats/imazing/design',
        ],
      },
    ],
  },
  {
    label: 'Instance internals',
    collapsed: true,
    items: [
      'docs/developer/reference/config-and-accounts',
      'docs/developer/reference/database',
      'docs/developer/reference/export-structure',
      'docs/developer/reference/export-formats',
      'docs/developer/reference/csv-columns',
      'docs/developer/reference/server-cli',
    ],
  },
];

export default defineConfig({
  site: 'https://messagecrate.app',
  redirects: {
    '/docs/developer/docker-compose/': '/docs/developer/docker/',
  },
  markdown: {
    processor: satteri({ hastPlugins: [wrapTables] }),
  },
  integrations: [
    mermaid({
      // The documentation is light only, so there is no theme to follow.
      theme: 'neutral',
      autoTheme: false,
      enableLog: false,
    }),
    starlight({
      title: 'Message Crate',
      description:
        'Extract messages from phone backups, import them into a server you run, and browse them in a website you control.',
      editLink: {
        baseUrl:
          'https://github.com/messagecrate/message-crate/edit/main/docs/',
      },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/messagecrate/message-crate',
        },
      ],
      head: [
        {
          tag: 'link',
          attrs: { rel: 'preconnect', href: 'https://fonts.googleapis.com' },
        },
        {
          tag: 'link',
          attrs: {
            rel: 'preconnect',
            href: 'https://fonts.gstatic.com',
            crossorigin: true,
          },
        },
        { tag: 'link', attrs: { rel: 'stylesheet', href: fontsHref } },
      ],
      customCss: ['./src/styles/custom.css'],
      // Light only, like the landing page: pin the theme and remove the
      // theme switch from the header.
      components: {
        ThemeProvider: './src/components/LightTheme.astro',
        ThemeSelect: './src/components/NoThemeSelect.astro',
      },
      expressiveCode: {
        themes: ['starlight-light'],
        // Code blocks: White with a Silver border, 8px radius, 13.5px mono,
        // 1.55 line height. Colours are the DESIGN.md tokens.
        styleOverrides: {
          borderRadius: '8px',
          borderWidth: '1px',
          borderColor: '#e5e7eb',
          codeBackground: '#ffffff',
          codeFontFamily:
            '"IBM Plex Mono", SFMono-Regular, Menlo, Consolas, monospace',
          codeFontSize: '0.84375rem',
          codeLineHeight: '1.55',
          codePaddingBlock: '0.875rem',
          codePaddingInline: '1rem',
          uiFontFamily: 'Inter, ui-sans-serif, system-ui, sans-serif',
          // The editor, terminal, and active-tab backgrounds are also set
          // in custom.css.
          frames: {
            editorTabBarBackground: '#f4f4f4',
            terminalTitlebarBackground: '#f4f4f4',
            editorActiveTabIndicatorTopColor: '#101010',
            editorActiveTabIndicatorBottomColor: 'transparent',
            frameBoxShadowCssValue: 'none',
          },
        },
      },
      plugins: [
        starlightSidebarTopics(
          [
            {
              label: 'User Guide',
              link: '/docs/user/',
              icon: 'open-book',
              items: userGuideItems,
            },
            {
              label: 'Developer',
              id: 'developer',
              link: '/docs/developer/',
              icon: 'laptop',
              items: developerItems,
            },
          ],
          {
            topics: {
              developer: [
                '/docs/developer/rustdoc',
                '/docs/developer/rustdoc/**',
              ],
            },
          },
        ),
      ],
    }),
  ],
});
