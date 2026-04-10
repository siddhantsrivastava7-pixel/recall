import { useEffect } from "react";
import { useAppStore } from "@/stores/appStore";

export const useBootApp = () => {
  const initialized = useAppStore((state) => state.initialized);
  const isBootstrapping = useAppStore((state) => state.isBootstrapping);
  const bootstrap = useAppStore((state) => state.bootstrap);

  useEffect(() => {
    if (!initialized && isBootstrapping) {
      void bootstrap();
    }
  }, [bootstrap, initialized, isBootstrapping]);
};
