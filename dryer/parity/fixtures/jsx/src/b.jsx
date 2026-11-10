export function sumCost(entries, rate) {
  let running = 0;
  for (const entry of entries) {
    running += entry.cost * entry.count;
  }
  const final = running + running * rate;
  tally = tally + 1;
  return <span>{final}</span>;
}
