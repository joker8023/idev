// [multica-tauri vendor patch] Analytics → no-op。
// 原实现是 posthog-js 包装（遥测发往 telemetry.multica.ai / PostHog 云端）。
// 本地版删除全部云端遥测：导出面保持不变（调用方一行不改），函数全部空实现，
// 不初始化、不发请求、不写 cookie。见 docs/MIGRATION.md（云端删除边界）。

export const EVENT_SCHEMA_VERSION = 2;

export interface AnalyticsConfig {
  key: string;
  host: string;
  appVersion?: string;
  environment?: string;
}

export type ClientType = "desktop" | "web";

export function detectClientType(): ClientType {
  return "desktop";
}

/** 本地版永远不初始化遥测，返回 false（= 未运行）。 */
export function initAnalytics(
  _config: AnalyticsConfig | null | undefined,
): boolean {
  return false;
}

export function identify(
  _userId: string,
  _userProperties?: Record<string, unknown>,
): void {
  // no-op
}

export function resetAnalytics(): void {
  // no-op
}

export interface CaptureEventOptions {
  sendInstantly?: boolean;
  onCaptured?: () => void;
}

export function captureEvent(
  _name: string,
  _props?: Record<string, unknown>,
  _options?: CaptureEventOptions,
): void {
  // no-op：本地版不发任何事件。
}

export function captureException(
  _error: unknown,
  _props?: Record<string, unknown>,
): void {
  // no-op
}

export function setPersonProperties(
  _props: Record<string, unknown>,
): void {
  // no-op
}

export function captureSignupSource(): void {
  // no-op：本地版不读 UTM / 不写 signup_source cookie。
}
