import type { ReactNode } from 'react';

export const PageHead = ({ title, children }: { title: string; children?: ReactNode }) => (
  <div className="page-head"><h1>{title}</h1><div className="tabs">{children}</div></div>
);
