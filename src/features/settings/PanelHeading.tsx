export function PanelHeading({ eyebrow, title, description, syncNote = true }: { eyebrow: string; title: string; description: string; syncNote?: boolean }) {
  return <header className="settings-panel-heading"><div><span>{eyebrow}</span><h2>{title}</h2><p>{description}</p></div>{syncNote && <small>自动同步到服务端，并在本机缓存</small>}</header>;
}
