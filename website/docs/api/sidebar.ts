import type { SidebarsConfig } from "@docusaurus/plugin-content-docs";

const sidebar: SidebarsConfig = {
  apisidebar: [
    {
      type: "doc",
      id: "api/bunny-api",
    },
    {
      type: "category",
      label: "Auth",
      link: {
        type: "doc",
        id: "api/auth",
      },
      items: [
        {
          type: "doc",
          id: "api/create-owner-account-first-run-only",
          label: "Create owner account (first run only)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/login-password-may-require-mfa-second-step",
          label: "Login (password; may require MFA second step)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/complete-login-with-totp-or-recovery-code",
          label: "Complete login with TOTP or recovery code",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/mfa-status-for-current-user",
          label: "MFA status for current user",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/start-mfa-enrollment-returns-otpauth-uri-and-manual-secret",
          label: "Start MFA enrollment (returns otpauth URI and manual secret)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/confirm-mfa-with-first-totp-code-returns-recovery-codes-once",
          label: "Confirm MFA with first TOTP code; returns recovery codes once",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/disable-mfa-recent-auth-totp-code",
          label: "Disable MFA (recent auth + TOTP code)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/regenerate-recovery-codes-recent-auth-totp-code",
          label: "Regenerate recovery codes (recent auth + TOTP code)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/logout",
          label: "Logout",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/current-user-includes-locale-en-fr",
          label: "Current user (includes locale en|fr)",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/update-current-user-preferences-locale-en-fr",
          label: "Update current user preferences (locale en|fr)",
          className: "api-method patch",
        },
      ],
    },
    {
      type: "category",
      label: "Sessions",
      link: {
        type: "doc",
        id: "api/sessions",
      },
      items: [
        {
          type: "doc",
          id: "api/list-sessions",
          label: "List sessions",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/create-session",
          label: "Create session",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/get-session",
          label: "Get session",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/join-session",
          label: "Join session",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/stop-session",
          label: "Stop session",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/reset-session",
          label: "Reset session",
          className: "api-method post",
        },
      ],
    },
    {
      type: "category",
      label: "Terminals",
      link: {
        type: "doc",
        id: "api/terminals",
      },
      items: [
        {
          type: "doc",
          id: "api/list-terminals",
          label: "List terminals",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/create-terminal",
          label: "Create terminal",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/terminal-web-socket",
          label: "Terminal WebSocket",
          className: "api-method get",
        },
      ],
    },
    {
      type: "category",
      label: "Previews",
      link: {
        type: "doc",
        id: "api/previews",
      },
      items: [
        {
          type: "doc",
          id: "api/create-port-preview",
          label: "Create port preview",
          className: "api-method post",
        },
      ],
    },
    {
      type: "category",
      label: "Browser",
      link: {
        type: "doc",
        id: "api/browser",
      },
      items: [
        {
          type: "doc",
          id: "api/create-browser-session",
          label: "Create browser session",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/web-rtc-browser-screencast-offer",
          label: "WebRTC browser screencast offer",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/browser-web-rtc-ice-candidate",
          label: "Browser WebRTC ICE candidate",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/stop-browser-web-rtc-stream",
          label: "Stop browser WebRTC stream",
          className: "api-method post",
        },
      ],
    },
    {
      type: "category",
      label: "Timeline",
      link: {
        type: "doc",
        id: "api/timeline",
      },
      items: [
        {
          type: "doc",
          id: "api/unified-timeline",
          label: "Unified timeline",
          className: "api-method get",
        },
      ],
    },
    {
      type: "category",
      label: "Voice",
      link: {
        type: "doc",
        id: "api/voice",
      },
      items: [
        {
          type: "doc",
          id: "api/voice-intent-proposal",
          label: "Voice intent proposal",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/confirm-voice-action",
          label: "Confirm voice action",
          className: "api-method post",
        },
      ],
    },
    {
      type: "category",
      label: "Push & WebRTC",
      link: {
        type: "doc",
        id: "api/push-webrtc",
      },
      items: [
        {
          type: "doc",
          id: "api/register-device-for-push-fcm",
          label: "Register device for push (FCM)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/unregister-push-device",
          label: "Unregister push device",
          className: "api-method delete",
        },
        {
          type: "doc",
          id: "api/ice-servers-and-web-rtc-availability",
          label: "ICE servers and WebRTC availability",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/web-rtc-sdp-offer-→-answer",
          label: "WebRTC SDP offer → answer",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/trickle-ice-candidate",
          label: "Trickle ICE candidate",
          className: "api-method post",
        },
      ],
    },
    {
      type: "category",
      label: "Secrets vault",
      link: {
        type: "doc",
        id: "api/secrets",
      },
      items: [
        {
          type: "doc",
          id: "api/vault-status-owner-only",
          label: "Vault status (owner only)",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/create-encrypted-vault-owner-only",
          label: "Create encrypted vault (owner only)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/unlock-vault-owner-only",
          label: "Unlock vault (owner only)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/lock-vault-owner-only",
          label: "Lock vault (owner only)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/list-secret-metadata-owner-only",
          label: "List secret metadata (owner only)",
          className: "api-method get",
        },
        {
          type: "doc",
          id: "api/create-or-update-secret-owner-only",
          label: "Create or update secret (owner only)",
          className: "api-method post",
        },
        {
          type: "doc",
          id: "api/remove-secret-owner-only",
          label: "Remove secret (owner only)",
          className: "api-method delete",
        },
        {
          type: "doc",
          id: "api/reveal-secret-value-owner-only",
          label: "Reveal secret value (owner only)",
          className: "api-method get",
        },
      ],
    },
    {
      type: "category",
      label: "Schemas",
      items: [
        {
          type: "doc",
          id: "api/schemas/bootstraprequest",
          label: "BootstrapRequest",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/loginrequest",
          label: "LoginRequest",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/mfaverifyrequest",
          label: "MfaVerifyRequest",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/recentauthrequest",
          label: "RecentAuthRequest",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/mfaenablerequest",
          label: "MfaEnableRequest",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/sessionresponse",
          label: "SessionResponse",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/terminalresponse",
          label: "TerminalResponse",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/apierror",
          label: "ApiError",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/vaultstatusresponse",
          label: "VaultStatusResponse",
          className: "schema",
        },
        {
          type: "doc",
          id: "api/schemas/secretmeta",
          label: "SecretMeta",
          className: "schema",
        },
      ],
    },
  ],
};

export default sidebar.apisidebar;
