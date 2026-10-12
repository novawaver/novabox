# Novabox

Novabox is a Windows desktop app for reading and sending mail from several Gmail accounts in one window. It talks to Google from this PC. Mail is cached in a local SQLite database. Refresh tokens are stored in Windows Credential Manager. There is no Novawave server in the path.

The browser preview (`npm run dev`) starts on a welcome screen. Choose **Explore the sample inbox** to try the layout before Google sign-in is configured. Nothing in that inbox is sent to Google.

## Run

```powershell
npm install
npm run tauri dev
```

## Google Cloud setup

While the OAuth app is in Testing, up to 100 test users can sign in. They will see an unverified-app warning, and sign-in expires about every 7 days.

1. Create a Google Cloud project and enable the Gmail API.
2. Configure the consent screen as External, in Testing. Add every Gmail address you will use as a test user.
3. Create an OAuth client of type **Desktop app**.
4. Add this authorized redirect URI: `http://127.0.0.1:47821/oauth/callback`
5. Request only `gmail.modify`, and the basic profile scopes (`openid`, `email`, `profile`).
6. In Novabox settings, paste the client ID and client secret, then use **Sign in with Google** once per account.

Removing an account deletes its local mail and revokes the saved token.

## What version 1 includes

- Account rail, unified inbox, and per-account folders
- Gmail categories: Primary, Promotions, Social, Updates
- Labels, star, archive, trash, spam, read/unread, and multi-select
- Thread view with remote images blocked until you show them
- Compose, reply, reply all, forward, From addresses, signatures, and drafts
- Local full-text search, plus Gmail search when you press Enter
- Background sync, tray icon, unread window title, optional start with Windows
- Light, dark, and system themes, and three densities

Snooze, scheduled send, and templates are included. A snoozed conversation leaves the inbox until the time you pick, then comes back on this PC. A scheduled message stays in Scheduled until Novabox sends it. Templates are saved on this PC.

Outlook and a paid account limit are left for a later version. During this private build, account limits are off.
