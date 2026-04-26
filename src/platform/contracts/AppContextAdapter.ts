import type { AppContextSnapshot, RuntimeInfo } from "@/domain/types";

export interface AppContextAdapter {
  getRuntimeInfo(): Promise<RuntimeInfo>;
  detectCurrentContext(): Promise<AppContextSnapshot>;
}
