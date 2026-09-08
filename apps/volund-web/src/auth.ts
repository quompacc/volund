import { ApiError, identityApi } from "./api";
import type { CurrentSession, SetupStatus } from "./types";

export async function requireSession(host: HTMLElement): Promise<CurrentSession> {
  const setup = await identityApi.setupStatus();
  if (!setup.initialized) await completeSetup(host, setup);
  const invitationToken = new URLSearchParams(window.location.search).get("invite");
  if (invitationToken) await acceptInvitation(host, invitationToken);
  try {
    return await requireCurrentPassword(await identityApi.current(), host);
  } catch (error) {
    if (!(error instanceof ApiError) || error.status !== 401) throw error;
    return requireCurrentPassword(await login(host), host);
  }
}

async function requireCurrentPassword(session: CurrentSession, host: HTMLElement): Promise<CurrentSession> {
  if (!session.mustChangePassword) return session;
  host.innerHTML = accessMarkup(
    "PASSWORTWECHSEL ERFORDERLICH",
    "Eigenes Passwort festlegen",
    "Das Startpasswort wurde administrativ vergeben. Ersetze es, bevor du VÖLUND verwendest.",
    `<label>Aktuelles Passwort<input name="currentPassword" type="password" autocomplete="current-password" required></label>
     <label>Neues Passwort<input name="newPassword" type="password" autocomplete="new-password" minlength="12" required></label>
     <label>Neues Passwort bestätigen<input name="confirmation" type="password" autocomplete="new-password" minlength="12" required></label>
     <button class="primary-action" type="submit">Passwort ersetzen</button>`,
  );
  await submitOnce(host.querySelector<HTMLFormElement>("form")!, async (data) => {
    const password = String(data.get("newPassword"));
    if (password !== String(data.get("confirmation"))) throw new Error("Die neuen Passwörter stimmen nicht überein.");
    await identityApi.changeOwnPassword(String(data.get("currentPassword")), password);
  });
  return { ...session, mustChangePassword: false };
}

async function acceptInvitation(host: HTMLElement, token: string): Promise<void> {
  host.innerHTML = accessMarkup(
    "EINLADUNG",
    "Konto aktivieren",
    "Lege dein persönliches Passwort fest. Der Einladungslink ist nur einmal verwendbar.",
    `<label>Passwort<input name="password" type="password" autocomplete="new-password" minlength="12" required></label>
     <label>Passwort bestätigen<input name="confirmation" type="password" autocomplete="new-password" minlength="12" required></label>
     <button class="primary-action" type="submit">Konto aktivieren</button>`,
  );
  await submitOnce(host.querySelector<HTMLFormElement>("form")!, async (data) => {
    const password = String(data.get("password"));
    if (password !== String(data.get("confirmation"))) throw new Error("Die Passwörter stimmen nicht überein.");
    await identityApi.acceptInvitation(token, password);
  });
  window.history.replaceState({}, "", `${window.location.pathname}?view=dashboard`);
}

async function completeSetup(host: HTMLElement, setup: SetupStatus): Promise<void> {
  host.innerHTML = accessMarkup(
    "ERSTE INBETRIEBNAHME",
    "Instanz absichern",
    setup.bootstrapAvailable
      ? "Lege den ersten Owner an. Der Bootstrap-Token stammt aus der geschützten Datei des Servers."
      : "Auf dem Server ist noch kein Bootstrap-Token konfiguriert.",
    setup.bootstrapAvailable
      ? `<label>Bootstrap-Token<input name="token" type="password" autocomplete="off" required minlength="32"></label>
         <label>Name<input name="displayName" autocomplete="name" required maxlength="160"></label>
         <label>E-Mail<input name="email" type="email" autocomplete="email" required></label>
         <label>Passwort<input name="password" type="password" autocomplete="new-password" required minlength="12"></label>
         <button class="primary-action" type="submit">Owner anlegen</button>`
      : '<p class="access-warning">Setze <code>VOLUND_BOOTSTRAP_TOKEN_FILE</code> und starte den Dienst neu.</p>',
  );
  if (!setup.bootstrapAvailable) return new Promise(() => undefined);
  const form = host.querySelector<HTMLFormElement>("form")!;
  await submitOnce(form, async (data) => {
    await identityApi.setupOwner(String(data.get("token")), {
      email: String(data.get("email")),
      displayName: String(data.get("displayName")),
      password: String(data.get("password")),
    });
  });
}

async function login(host: HTMLElement): Promise<CurrentSession> {
  host.innerHTML = accessMarkup(
    "GESCHÜTZTE INSTANZ",
    "Anmelden",
    "Melde dich mit deinem lokalen VÖLUND-Konto an.",
    `<label>E-Mail<input name="email" type="email" autocomplete="username" required autofocus></label>
     <label>Passwort<input name="password" type="password" autocomplete="current-password" required></label>
     <button class="primary-action" type="submit">Anmelden</button>`,
  );
  const form = host.querySelector<HTMLFormElement>("form")!;
  await submitOnce(form, async (data) => {
    await identityApi.login(String(data.get("email")), String(data.get("password")));
  });
  return identityApi.current();
}

function accessMarkup(eyebrow: string, title: string, copy: string, fields: string): string {
  return `<main class="access-shell"><section class="access-card">
    <div class="access-brand"><span class="brand-mark">V</span><div><strong>VÖLUND</strong><small>THE SOVEREIGN CAD VAULT</small></div></div>
    <p class="eyebrow">${eyebrow}</p><h1>${title}</h1><p>${copy}</p>
    <form><div class="access-error" role="alert"></div>${fields}</form>
  </section></main>`;
}

function submitOnce(form: HTMLFormElement, action: (data: FormData) => Promise<void>): Promise<void> {
  return new Promise((resolve) => {
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      const button = form.querySelector<HTMLButtonElement>("button[type=submit]");
      const error = form.querySelector<HTMLElement>(".access-error")!;
      if (button) button.disabled = true;
      error.textContent = "";
      void action(new FormData(form)).then(resolve, (reason: unknown) => {
        error.textContent = reason instanceof Error ? reason.message : "Vorgang fehlgeschlagen";
        if (button) button.disabled = false;
      });
    });
  });
}
