import { defineConfig } from 'astro/config';
import starlight from '@astrojs/starlight';
import starlightSidebarTopics from 'starlight-sidebar-topics';
import mermaid from 'astro-mermaid';
import { satteri } from '@astrojs/markdown-satteri';
import { wrapTables } from './src/lib/satteri-wrap-tables.mjs';

// Guidebook type: a display face for headings, a text face for body copy,
// and a mono face for every typed token. See src/styles/custom.css.
const fontsHref =
  'https://fonts.googleapis.com/css2' +
  '?family=Bricolage+Grotesque:opsz,wght@12..96,500..700' +
  '&family=IBM+Plex+Sans:ital,wght@0,400;0,500;0,600;1,400' +
  '&family=IBM+Plex+Mono:wght@400;500' +
  '&display=swap';

const dark = (theme) => theme.type === 'dark';

const limitedBadge = {
  text: 'Limited',
  variant: 'caution',
};

const userGuideItems = [
  { label: 'Home', slug: 'vault/user' },
  {
    label: 'Get started',
    items: [
      'vault/user/get-started/what-is-message-vault',
      'vault/user/get-started/why-you-provide-backups',
      'vault/user/get-started/try-the-vault',
      'vault/user/get-started/your-own-messages',
      'vault/user/get-started/install-the-desktop-app',
    ],
  },
  {
    label: 'Prepare a backup',
    items: [
      'vault/user/prepare-a-backup',
      'vault/user/prepare-a-backup/iphone-ipad',
      'vault/user/prepare-a-backup/iphone-whatsapp',
      'vault/user/prepare-a-backup/android-sms',
      'vault/user/prepare-a-backup/android-whatsapp',
    ],
  },
  'vault/user/import-from-a-backup',
  'vault/user/browse-your-messages',
  {
    label: 'How do I…',
    items: [
      'vault/user/how-to/search',
      'vault/user/how-to/contacts-and-labels',
      'vault/user/how-to/saved-searches',
      'vault/user/how-to/trash',
      'vault/user/how-to/settings',
      'vault/user/how-to/export-from-the-vault',
      'vault/user/how-to/convert-formats',
      'vault/user/how-to/media-and-privacy',
      { slug: 'vault/user/how-to/rescue-imports', badge: limitedBadge },
      'vault/user/how-to/update',
      'vault/user/how-to/troubleshooting',
    ],
  },
  'vault/user/glossary',
];

const developerItems = [
  'vault/developer',
  'vault/developer/contributing',
  'vault/developer/release',
  'vault/developer/rustdoc-style',
  {
    label: 'Architecture',
    items: [
      'vault/developer/vault-design',
      'vault/developer/message-transfer',
      'vault/developer/architecture/common-message',
    ],
  },
  'vault/developer/docker',
  'vault/developer/reference/api',
  {
    label: 'HTTP problem types',
    collapsed: true,
    autogenerate: { directory: 'vault/developer/reference/errors' },
  },
  {
    label: 'HTTP API reference',
    link: '/vault/developer/rustdoc/http/',
    attrs: { target: '_self' },
  },
  {
    label: 'Rust crate docs',
    link: '/vault/developer/rustdoc/',
    attrs: { target: '_self' },
  },
  {
    label: 'Formats',
    items: [
      'vault/developer/formats',
      'vault/developer/formats/mail-archive',
      'vault/developer/formats/sms-backup-restore-xml',
      'vault/developer/formats/convert',
      {
        label: 'SMS Backup & Restore',
        items: [
          'vault/developer/formats/sms-backup-restore/input',
          'vault/developer/formats/sms-backup-restore/mapping',
        ],
      },
      {
        label: 'SMS Backup+',
        items: [
          'vault/developer/formats/sms-backup-plus/format',
          'vault/developer/formats/sms-backup-plus/mapping',
        ],
      },
      {
        label: 'GO SMS Pro',
        items: ['vault/developer/formats/go-sms-pro/mapping'],
      },
      {
        label: 'iMazing',
        items: [
          'vault/developer/formats/imazing/input',
          'vault/developer/formats/imazing/design',
        ],
      },
    ],
  },
  {
    label: 'Instance internals',
    collapsed: true,
    items: [
      'vault/developer/reference/config-and-accounts',
      'vault/developer/reference/database',
      'vault/developer/reference/export-structure',
      'vault/developer/reference/export-formats',
      'vault/developer/reference/csv-columns',
      'vault/developer/reference/server-cli',
    ],
  },
];

export default defineConfig({
  site: 'https://bitrealm.io',
  redirects: {
    '/vault/developer/docker-compose/': '/vault/developer/docker/',
  },
  markdown: {
    processor: satteri({ hastPlugins: [wrapTables] }),
  },
  integrations: [
    mermaid({
      autoTheme: true,
      enableLog: false,
    }),
    starlight({
      title: 'Message Vault',
      description:
        'Extract messages from phone backups, import them into a local vault, and browse them in a website you control.',
      editLink: {
        baseUrl:
          'https://github.com/bitrealm-io/message-vault/edit/main/docs/',
      },
      social: [
        {
          icon: 'github',
          label: 'GitHub',
          href: 'https://github.com/bitrealm-io/message-vault',
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
      expressiveCode: {
        // Code blocks: soft border, 6px radius, 13.5px mono, 1.55 line
        // height, on the code-block ground from custom.css.
        styleOverrides: {
          borderRadius: '6px',
          borderWidth: '1px',
          borderColor: (ctx) => (dark(ctx.theme) ? '#232b2f' : '#e6eaec'),
          codeBackground: (ctx) => (dark(ctx.theme) ? '#10161a' : '#eef1f0'),
          codeFontFamily:
            '"IBM Plex Mono", SFMono-Regular, Menlo, Consolas, monospace',
          codeFontSize: '0.84375rem',
          codeLineHeight: '1.55',
          codePaddingBlock: '0.875rem',
          codePaddingInline: '1rem',
          uiFontFamily: '"IBM Plex Sans", "Segoe UI", Roboto, sans-serif',
          // The editor, terminal, and active-tab backgrounds are set in
          // custom.css: Starlight's own theme pins them and wins here.
          frames: {
            editorTabBarBackground: (ctx) =>
              dark(ctx.theme) ? '#1b2124' : '#f6f7f5',
            terminalTitlebarBackground: (ctx) =>
              dark(ctx.theme) ? '#1b2124' : '#f6f7f5',
            editorActiveTabIndicatorTopColor: (ctx) =>
              dark(ctx.theme) ? '#63c7ce' : '#0e6b73',
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
              link: '/vault/user/',
              icon: 'open-book',
              items: userGuideItems,
            },
            {
              label: 'Developer',
              id: 'developer',
              link: '/vault/developer/',
              icon: 'laptop',
              items: developerItems,
            },
          ],
          {
            topics: {
              developer: [
                '/vault/developer/rustdoc',
                '/vault/developer/rustdoc/**',
              ],
            },
          },
        ),
      ],
    }),
  ],
});
