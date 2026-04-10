import type { RuntimePlatform } from "@/domain/types";
import type { PlatformAdapters } from "@/platform/contracts/PlatformAdapters";
import { createPlatformAdapters } from "@/platform/factory/createPlatformAdapters";

let runtimePlatform: RuntimePlatform = "windows";
let platformAdapters: PlatformAdapters = createPlatformAdapters("windows");

export const configureRuntimePlatform = (platform: RuntimePlatform) => {
  runtimePlatform = platform;
  platformAdapters = createPlatformAdapters(platform);
};

export const getRuntimePlatform = () => runtimePlatform;
export const getPlatformAdapters = () => platformAdapters;
