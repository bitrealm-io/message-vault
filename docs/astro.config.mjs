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

const advancedBadge = {
  text: 'Advanced',
  variant: 'note',
};

const userGuideItems = [
  { label: 'Home', slug: 'docs/user' },
  {
    label: 'Get started',
    items: [
      { label: '1. What Message Crate is', slug: 'docs/user/get-started/what-is-message-crate' },
      { label: '2. Start a Message Crate', slug: 'docs/user/get-started/start-a-message-crate' },
      { label: '3. Look around the demo data', slug: 'docs/user/get-started/look-around-the-demo-data' },
      { label: '4. Create the Owner and an account', slug: 'docs/user/get-started/create-the-owner-and-an-account' },
      { label: '5. Install the desktop app', slug: 'docs/user/get-started/install-the-desktop-app' },
      { label: '6. Back up an iPhone', slug: 'docs/user/get-started/back-up-an-iphone' },
      { label: '6. Back up an Android phone', slug: 'docs/user/get-started/back-up-an-android-phone' },
      { label: '7. Import your backup', slug: 'docs/user/get-started/import-your-backup' },
      { label: '8. Where to go next', slug: 'docs/user/get-started/where-to-go-next' },
    ],
  },
  {
    label: 'Other import sources',
    items: [
      'docs/user/import-sources/mac-messages',
      { slug: 'docs/user/import-sources/whatsapp', badge: advancedBadge },
      { slug: 'docs/user/import-sources/old-backups', badge: limitedBadge },
    ],
  },
  {
    label: 'Features',
    items: [
      {
        label: 'Messages',
        items: [
          'docs/user/features/messages/browse',
          'docs/user/features/messages/search',
          'docs/user/features/messages/saved-searches',
          'docs/user/features/messages/message-tags',
          'docs/user/features/messages/trash',
          'docs/user/features/messages/import',
          'docs/user/features/messages/export',
          'docs/user/features/messages/attachments-and-media',
        ],
      },
      {
        label: 'Contacts',
        items: [
          'docs/user/features/contacts/contacts',
          'docs/user/features/contacts/contact-groups',
          'docs/user/features/contacts/unknown',
        ],
      },
      {
        label: 'Settings',
        items: [
          'docs/user/features/settings/account-and-profile',
          'docs/user/features/settings/storage',
          'docs/user/features/settings/system',
          'docs/user/features/settings/convert',
          'docs/user/features/settings/appearance',
        ],
      },
      {
        label: 'Owner',
        items: [
          'docs/user/features/owner/owner-home',
          'docs/user/features/owner/update',
          'docs/user/features/owner/run-on-another-machine',
          'docs/user/features/owner/troubleshooting',
        ],
      },
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
