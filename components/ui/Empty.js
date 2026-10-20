export default function Empty({ icon, title, hint, children }) {
  return (
    <div className="empty">
      <div className="empty-ico">{icon}</div>
      <b>{title}</b>
      <p className="muted">{hint}</p>
      {children}
    </div>
  );
}
