import { createContext, useContext } from 'react';

/** What the admin shell offers its pages: a place in its status bar. */
export interface AdminShellApi {
  setCursor: (text: string) => void;
}

export const AdminShellContext = createContext<AdminShellApi>({ setCursor: () => {} });
export const useAdminShell = (): AdminShellApi => useContext(AdminShellContext);
