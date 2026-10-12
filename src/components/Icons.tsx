export function LogoMark({ className = "h-7 w-7" }: { className?: string }) {
  return <img src="/logo.png" alt="" className={`${className} object-contain`} />;
}

export function GoogleMark() {
  return (
    <svg width="18" height="18" viewBox="0 0 48 48" aria-hidden="true">
      <path fill="#FFC107" d="M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.3 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3.1 0 5.8 1.2 8 3.1l5.7-5.7C34.2 6.1 29.4 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.2-.1-2.3-.4-3.5z" />
      <path fill="#FF3D00" d="M6.3 14.7l6.6 4.8C14.7 16 19 12 24 12c3.1 0 5.8 1.2 8 3.1l5.7-5.7C34.2 6.1 29.4 4 24 4 16.3 4 9.6 8.3 6.3 14.7z" />
      <path fill="#4CAF50" d="M24 44c5.2 0 10-2 13.6-5.2l-6.3-5.3C29.3 35.1 26.8 36 24 36c-5.3 0-9.7-3.3-11.3-8l-6.5 5C9.5 39.6 16.2 44 24 44z" />
      <path fill="#1976D2" d="M43.6 20.5H42V20H24v8h11.3c-1 2.9-3.1 5.2-5.9 6.5l6.3 5.3C38.2 37.3 44 32 44 24c0-1.2-.1-2.3-.4-3.5z" />
    </svg>
  );
}

export function IconSearch() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><circle cx="11" cy="11" r="7" /><path d="m20 20-3.5-3.5" /></svg>;
}
export function IconCompose() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="M12 20h9" /><path d="M16.5 3.5a2.1 2.1 0 0 1 3 3L8 18l-4 1 1-4Z" /></svg>;
}
export function IconRefresh() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="M21 12a9 9 0 1 1-2.6-6.4L21 8" /><path d="M21 3v5h-5" /></svg>;
}
export function IconArchive() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="M3 7h18" /><path d="M5 7v12h14V7" /><path d="M10 11h4" /></svg>;
}
export function IconTrash() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="M4 7h16" /><path d="M9 7V4h6v3" /><path d="M8 7l1 13h6l1-13" /></svg>;
}
export function IconStar({ filled = false }: { filled?: boolean }) {
  return <svg viewBox="0 0 24 24" className={`h-4 w-4 ${filled ? "fill-star text-star" : ""}`} fill={filled ? "currentColor" : "none"} stroke="currentColor" strokeWidth="2"><path d="m12 3 2.7 5.5L21 9.2l-4.5 4.4 1.1 6.4L12 17l-5.6 3 1.1-6.4L3 9.2l6.3-.7L12 3Z" /></svg>;
}
export function IconMail() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="M4 6h16v12H4z" /><path d="m4 7 8 6 8-6" /></svg>;
}
export function IconPaperclip() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="m16 8-6.5 6.5a2.5 2.5 0 0 1-3.5-3.5L13 4a4 4 0 0 1 5.7 5.7l-7.2 7.2a5.5 5.5 0 0 1-7.8-7.8L11 2" /></svg>;
}
export function IconBack() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="M15 6 9 12l6 6" /></svg>;
}
export function IconNext() {
  return <svg viewBox="0 0 24 24" className="h-4 w-4" fill="none" stroke="currentColor" strokeWidth="2"><path d="m9 6 6 6-6 6" /></svg>;
}

type IconProps = { className?: string };
const stroke = { fill: "none", stroke: "currentColor", strokeWidth: 2, strokeLinecap: "round", strokeLinejoin: "round" } as const;

export function IconMenu({ className = "h-5 w-5" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M4 6h16M4 12h16M4 18h16" /></svg>;
}
export function IconInbox({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M22 12h-6l-2 3h-4l-2-3H2" /><path d="M5.5 5.1 2 12v6a2 2 0 0 0 2 2h16a2 2 0 0 0 2-2v-6l-3.5-6.9A2 2 0 0 0 16.8 4H7.2a2 2 0 0 0-1.7 1.1Z" /></svg>;
}
export function IconStarOutline({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="m12 3 2.7 5.5L21 9.2l-4.5 4.4 1.1 6.4L12 17l-5.6 3 1.1-6.4L3 9.2l6.3-.7L12 3Z" /></svg>;
}
export function IconSend({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M22 2 11 13" /><path d="M22 2 15 22l-4-9-9-4 20-7Z" /></svg>;
}
export function IconDraft({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8Z" /><path d="M14 2v6h6" /><path d="M8 13h8M8 17h5" /></svg>;
}
export function IconClock({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><circle cx="12" cy="12" r="9" /><path d="M12 7v5l3 2" /></svg>;
}
export function IconScheduled({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><rect x="3" y="4" width="18" height="17" rx="2" /><path d="M16 2v4M8 2v4M3 10h18" /><path d="M12 14v3l2 1" /></svg>;
}
export function IconSpam({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M7.9 2h8.2L22 7.9v8.2L16.1 22H7.9L2 16.1V7.9Z" /><path d="M12 8v4M12 16h.01" /></svg>;
}
export function IconBin({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M3 6h18" /><path d="M8 6V4h8v2" /><path d="M19 6l-1 14a2 2 0 0 1-2 2H8a2 2 0 0 1-2-2L5 6" /><path d="M10 11v6M14 11v6" /></svg>;
}
export function IconLabel({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M20.6 13.4 13.4 20.6a2 2 0 0 1-2.8 0L3 13V3h10l7.6 7.6a2 2 0 0 1 0 2.8Z" /><circle cx="7.5" cy="7.5" r="1.5" /></svg>;
}
export function IconPlus({ className = "h-5 w-5" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke} strokeWidth={2.5}><path d="M12 5v14M5 12h14" /></svg>;
}
export function IconX({ className = "h-5 w-5" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke} strokeWidth={2.5}><path d="M18 6 6 18M6 6l12 12" /></svg>;
}
export function IconSync({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M21 12a9 9 0 0 1-15.5 6.2L3 16" /><path d="M3 21v-5h5" /><path d="M3 12a9 9 0 0 1 15.5-6.2L21 8" /><path d="M21 3v5h-5" /></svg>;
}
export function IconCloudOff({ className = "h-[18px] w-[18px]" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="m2 2 20 20" /><path d="M5.8 5.8A7 7 0 0 0 6.5 19H17a4.5 4.5 0 0 0 2.4-.7" /><path d="M21.5 15.5A4.5 4.5 0 0 0 17.5 10h-.3A7 7 0 0 0 10 5.1" /></svg>;
}
export function IconSettings({ className = "h-5 w-5" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M12.2 2h-.4a2 2 0 0 0-2 2v.2a2 2 0 0 1-1 1.7l-.4.3a2 2 0 0 1-2 0l-.2-.1a2 2 0 0 0-2.7.7l-.2.4a2 2 0 0 0 .7 2.7l.2.1a2 2 0 0 1 1 1.7v.5a2 2 0 0 1-1 1.8l-.2.1a2 2 0 0 0-.7 2.7l.2.4a2 2 0 0 0 2.7.7l.2-.1a2 2 0 0 1 2 0l.4.3a2 2 0 0 1 1 1.7v.2a2 2 0 0 0 2 2h.4a2 2 0 0 0 2-2v-.2a2 2 0 0 1 1-1.7l.4-.3a2 2 0 0 1 2 0l.2.1a2 2 0 0 0 2.7-.7l.2-.4a2 2 0 0 0-.7-2.7l-.2-.1a2 2 0 0 1-1-1.8v-.5a2 2 0 0 1 1-1.7l.2-.1a2 2 0 0 0 .7-2.7l-.2-.4a2 2 0 0 0-2.7-.7l-.2.1a2 2 0 0 1-2 0l-.4-.3a2 2 0 0 1-1-1.7V4a2 2 0 0 0-2-2Z" /><circle cx="12" cy="12" r="3" /></svg>;
}
export function IconBold({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke} strokeWidth={2.5}><path d="M7 5h6a3.5 3.5 0 0 1 0 7H7z" /><path d="M7 12h7a3.5 3.5 0 0 1 0 7H7z" /></svg>;
}
export function IconItalic({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M19 4h-9M14 20H5M15 4 9 20" /></svg>;
}
export function IconUnderline({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M6 4v6a6 6 0 0 0 12 0V4" /><path d="M4 20h16" /></svg>;
}
export function IconStrike({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M16 5.5A4.5 3.5 0 0 0 12 4c-2.5 0-4.5 1.3-4.5 3.2 0 1.6 1.2 2.4 3 2.9" /><path d="M4 12h16" /><path d="M8 18.5A5 3.5 0 0 0 12 20c2.6 0 4.6-1.3 4.6-3.3 0-.9-.4-1.6-1-2.2" /></svg>;
}
export function IconListBullet({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M9 6h11M9 12h11M9 18h11" /><circle cx="4.5" cy="6" r="1" fill="currentColor" /><circle cx="4.5" cy="12" r="1" fill="currentColor" /><circle cx="4.5" cy="18" r="1" fill="currentColor" /></svg>;
}
export function IconListNumber({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M10 6h10M10 12h10M10 18h10" /><path d="M4 5l1.5-1v5" strokeWidth={1.6} /><path d="M3.5 14.2a1.4 1.4 0 0 1 2.6.6c0 1-2.6 2.2-2.6 3.2h2.8" strokeWidth={1.6} /></svg>;
}
export function IconLink({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M10 13a5 5 0 0 0 7.5.5l3-3a5 5 0 0 0-7-7l-1.7 1.7" /><path d="M14 11a5 5 0 0 0-7.5-.5l-3 3a5 5 0 0 0 7 7l1.7-1.7" /></svg>;
}
export function IconClearFormat({ className = "h-4 w-4" }: IconProps) {
  return <svg viewBox="0 0 24 24" className={className} {...stroke}><path d="M4 7V4h16v3" /><path d="M5 20h6" /><path d="M13 4 8 20" /><path d="m15 15 5 5M20 15l-5 5" /></svg>;
}
