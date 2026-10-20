'use client';
import Logo from '@/components/ui/Logo';

export default function PageHeader({ title, subtitle, children }) {
  return (
    <header className="page-head">
      <div>
        <Logo size={18} className="small" />
        <h2>{title}</h2>
        {subtitle && <p className="muted">{subtitle}</p>}
      </div>
      <div className="page-actions">{children}</div>
    </header>
  );
}
