import { invoke } from "@tauri-apps/api/core";
import type {
  Account,
  AppInfo,
  ComposeDraft,
  InboxTabs,
  Mailbox,
  MailQuery,
  Notice,
  SendResult,
  Settings,
  ThreadDetail,
  ThreadSummary,
  Template,
  ThreadTarget,
  UpdateInfo,
} from "./types";
import { defaultInboxTabs } from "./types";
import { inTauri } from "./platform";

export type ImportItem = { email: string; status: "added" | "updated" | "failed"; message?: string | null };

export type PopupSnapshot = { version: number; corner: string; notices: Notice[] };

function assertDesktop(): void {
  if (!inTauri()) throw new Error("Sign-in and live mail run in the Novabox desktop app.");
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  settingsGet: () => invoke<Settings>("settings_get"),
  notifyShow: (title: string, body: string) => invoke<void>("notify_show", { title, body }),
  popupPush: (notice: Notice, corner: string, max: number) => invoke<void>("popup_push", { notice, corner, max }),
  popupList: () => invoke<PopupSnapshot>("popup_list"),
  popupLayout: (height: number) => invoke<void>("popup_layout", { height }),
  popupDismiss: (id: string) => invoke<void>("popup_dismiss", { id }),
  popupOpen: (notice: Notice) => invoke<void>("popup_open", { notice }),
  settingsSave: (settings: Settings) => invoke<void>("settings_save", { settings }),
  mailbox: (query: MailQuery, inboxTabs: InboxTabs = defaultInboxTabs) =>
    invoke<Mailbox>("mailbox_query", {
      query: {
        accountId: query.accountId,
        folder: query.folder,
        labelId: query.labelId,
        category: query.folder === "inbox" ? query.category : null,
        search: query.search.trim() ? query.search : null,
        limit: query.limit,
        offset: query.offset,
        inboxTabs,
      },
    }),
  threadGet: (accountId: string, threadId: string) =>
    invoke<ThreadDetail>("thread_get", { accountId, threadId }),
  signIn: async () => {
    assertDesktop();
    return invoke<Account>("sign_in");
  },
  signInCancel: () => invoke<void>("sign_in_cancel"),
  accountRemove: (accountId: string, revoke = true) => invoke<void>("account_remove", { accountId, revoke }),
  backupExport: (path: string, password: string) => invoke<{ exported: string[]; skipped: string[] }>("backup_export", { path, password }),
  backupImport: (path: string, password: string) => invoke<ImportItem[]>("backup_import", { path, password }),
  setSignature: (accountId: string, signature: string) =>
    invoke<void>("account_set_signature", { update: { accountId, signature } }),
  threadsModify: (targets: ThreadTarget[], action: string, labelId?: string) =>
    invoke<void>("threads_modify", { batch: { targets, action, labelId: labelId ?? null } }),
  mailSend: (draft: ComposeDraft) => invoke<SendResult>("mail_send", { request: composePayload(draft) }),
  draftSave: (draft: ComposeDraft) => invoke<SendResult>("draft_save", { request: composePayload(draft) }),
  draftDelete: (accountId: string, draftId: string) => invoke<void>("draft_delete", { accountId, draftId }),
  remoteSearch: (accountId: string | null, query: string) =>
    invoke<ThreadSummary[]>("remote_search", { accountId, query }),
  syncNow: () => invoke<void>("sync_now"),
  checkConnection: () => invoke<void>("check_connection"),
  updateCheck: () => invoke<UpdateInfo | null>("update_check"),
  updateInstall: () => invoke<void>("update_install"),
  attachmentGet: (accountId: string, messageId: string, attachmentId: string) =>
    invoke<{ filename: string; mimeType: string; dataBase64: string }>("attachment_get", {
      request: { accountId, messageId, attachmentId },
    }),
  attachmentSave: (accountId: string, messageId: string, attachmentId: string, path: string) =>
    invoke<void>("attachment_save", {
      request: { accountId, messageId, attachmentId, path },
    }),
  threadsSnooze: (targets: ThreadTarget[], untilAt: number) => invoke<void>("threads_snooze", { targets, untilAt }),
  threadsUnsnooze: (targets: ThreadTarget[]) => invoke<void>("threads_unsnooze", { targets }),
  mailSchedule: (draft: ComposeDraft, sendAt: number) => invoke<string>("mail_schedule", { request: composePayload(draft), sendAt }),
  scheduleCancel: (id: string) => invoke<void>("schedule_cancel", { id }),
  scheduleSendNow: (id: string) => invoke<SendResult>("schedule_send_now", { id }),
  templatesList: () => invoke<Template[]>("templates_list"),
  templateSave: (template: Template) => invoke<Template>("template_save", { template }),
  templateDelete: (id: string) => invoke<void>("template_delete", { id }),
};

function composePayload(draft: ComposeDraft) {
  return {
    accountId: draft.accountId,
    fromEmail: draft.fromEmail,
    to: draft.to,
    cc: draft.cc,
    bcc: draft.bcc,
    subject: draft.subject,
    html: draft.html,
    attachments: draft.attachments.map((file) => ({
      filename: file.filename,
      mimeType: file.mimeType,
      dataBase64: file.dataBase64,
    })),
    threadId: draft.threadId ?? null,
    inReplyTo: draft.inReplyTo ?? null,
    references: draft.references ?? null,
    draftId: draft.draftId ?? null,
  };
}
