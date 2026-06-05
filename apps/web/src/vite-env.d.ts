/// <reference types="vite/client" />

declare module '@bunny/i18n/messages.json' {
  const value: Record<string, { en: string; fr: string }>;
  export default value;
}

declare module '@bunny/i18n/web-messages.json' {
  const value: Record<string, { en: string; fr: string }>;
  export default value;
}
