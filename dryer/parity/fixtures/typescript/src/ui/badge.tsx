export function Badge(props: { label: string; count: number }): JSX.Element {
  const text = props.label.toUpperCase();
  const total = props.count + 1;
  const title = text + " " + total;
  return <span className="badge">{title}</span>;
}
