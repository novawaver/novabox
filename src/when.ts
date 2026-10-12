export type WhenChoice = { label: string; at: number };

function atHour(day: Date, hour: number): Date {
  const next = new Date(day);
  next.setHours(hour, 0, 0, 0);
  return next;
}

export function whenChoices(now = new Date()): WhenChoice[] {
  const choices: WhenChoice[] = [];
  const later = atHour(now, 18);
  if (later.getTime() > now.getTime() + 30 * 60 * 1000) {
    choices.push({ label: `Later today, ${clock(later)}`, at: later.getTime() });
  }
  const tomorrow = atHour(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1), 8);
  choices.push({ label: `Tomorrow, ${clock(tomorrow)}`, at: tomorrow.getTime() });
  const monday = new Date(now);
  const days = (8 - monday.getDay()) % 7 || 7;
  monday.setDate(monday.getDate() + days);
  const nextWeek = atHour(monday, 8);
  choices.push({ label: `Next week, ${nextWeek.toLocaleDateString([], { weekday: "short", month: "short", day: "numeric" })}`, at: nextWeek.getTime() });
  return choices;
}

export function tomorrowMorning(now = new Date()): number {
  return atHour(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1), 8).getTime();
}

function clock(date: Date): string {
  return date.toLocaleTimeString([], { hour: "numeric", minute: "2-digit" });
}
