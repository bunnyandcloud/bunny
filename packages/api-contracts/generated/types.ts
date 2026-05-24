/**
 * Generated from openapi.yaml — run `npm run generate:ts` in packages/api-contracts
 */
export type ApiVersion = 'v1';

export interface BootstrapRequest {
  email: string;
  password: string;
}

export interface LoginRequest {
  email: string;
  password: string;
  device_id?: string;
}

export interface SessionResponse {
  id: string;
  login_url: string;
  auth_required: boolean;
}

export interface TerminalResponse {
  id: string;
  name: string;
  ws_url: string;
}

export interface ApiErrorBody {
  code: string;
  message: string;
  details?: unknown;
}
