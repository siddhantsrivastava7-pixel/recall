import type { LicenseState, ServiceResult } from "@/domain/types";

export interface LicenseService {
  getState(): Promise<LicenseState>;
  activate(licenseKey: string): Promise<ServiceResult<LicenseState>>;
  deactivate(): Promise<ServiceResult<LicenseState>>;
}
