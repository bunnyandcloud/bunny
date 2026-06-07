/// <reference types="vite/client" />

declare module '*.png' {
  const src: string;
  export default src;
}

declare module '@bunny/i18n/messages.json' {
  const value: Record<string, { en: string; fr: string }>;
  export default value;
}

declare module '@bunny/i18n/web-messages.json' {
  const value: Record<string, { en: string; fr: string }>;
  export default value;
}
