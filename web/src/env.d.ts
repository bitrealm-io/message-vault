// Ambient declarations for asset modules that have no runtime typings.
// TypeScript 7 requires type declarations for side-effect imports such as
// `import "./theme.css"`, which Vite handles at build time.
declare module "*.css";

/** This app's Build, substituted by `vite.config.ts`; read it from `lib/build.ts`. */
declare const __APP_BUILD__: string;
