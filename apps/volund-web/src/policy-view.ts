import { policyApi } from "./api";
import { confirmExact } from "./confirmation";
import type { ConversionProfile, ManagedLibrary, RetentionPreview, ScanSchedule } from "./types";

export function policyPanels(profiles: ConversionProfile[], schedules: ScanSchedule[], retention: RetentionPreview, libraries: ManagedLibrary[], timeZone: string): string {
  const libraryOptions=libraries.filter((library)=>library.enabled).map((library)=>`<option value="${esc(library.id)}">${esc(library.name)}</option>`).join("");
  return `<section class="admin-section"><div class="section-heading"><div><p class="eyebrow">AUSFÜHRUNGSPOLITIK</p><h2>Konvertierungsprofile</h2></div><span>${profiles.length}</span></div>
    <form id="create-profile" class="admin-form"><label><span>Profilname</span><input name="name" maxlength="80" required></label><label><span>Native Voreinstellung</span><select name="nativePreset"><option>web</option><option>fine</option></select></label><label><span>Lineare Abweichung (optional)</span><input name="linearDeflection" type="number" min="0.000001" max="1000" step="any"></label><label><span>Winkelabweichung (optional)</span><input name="angularDeflection" type="number" min="0.01" max="3.141592653589793" step="any"></label><button class="primary-action">Profil anlegen</button></form>
    <div class="admin-list">${profiles.map((profile)=>`<article><div><strong>${esc(profile.name)}</strong><small>${profile.nativePreset} · Revision ${profile.revision}${profile.enabled?" · aktiv":" · deaktiviert"}${profile.builtIn?" · sicherer Systemstandard":""}</small></div><button class="secondary-action" data-toggle-profile='${data(profile)}'${profile.builtIn?' disabled title="Systemstandard bleibt unverändert"':""}>${profile.enabled?"Deaktivieren":"Aktivieren"}</button><button class="secondary-action" data-retire-profile="${profile.id}" data-policy-revision="${profile.revision}"${profile.enabled&&!profile.builtIn?"":' disabled title="Systemstandard oder bereits stillgelegt"'}>Stilllegen</button></article>`).join("")||'<p class="admin-copy">Keine Profile vorhanden.</p>'}</div></section>
    <section class="admin-section"><div class="section-heading"><div><p class="eyebrow">ZEITPLÄNE</p><h2>Bibliotheksscans</h2></div><span>${schedules.length}</span></div>
    ${libraryOptions?`<form id="create-schedule" class="admin-form"><label><span>Bibliothek</span><select name="libraryId">${libraryOptions}</select></label><label><span>Zeitplanname</span><input name="name" maxlength="80" required></label><label><span>Lokale Uhrzeit</span><input name="localTime" type="time" value="02:00" required></label><label><span>IANA-Zeitzone</span><input name="timeZone" value="${esc(timeZone)}" required></label><label><span>Wochentagmaske</span><input name="weekdayMask" type="number" min="1" max="127" value="127" required></label><label class="checkbox-label"><input name="fullScan" type="checkbox"> Vollscan</label><button class="primary-action">Zeitplan anlegen</button></form>`:'<p class="admin-message error">Keine aktive Bibliothek; Zeitplanerstellung ist blockiert.</p>'}
    <div class="admin-list">${schedules.map((schedule)=>`<article><div><strong>${esc(schedule.name)}</strong><small>${esc(schedule.libraryName)} · ${schedule.localTime} ${esc(schedule.timeZone)} · ${schedule.lastOutcome??"noch nicht ausgeführt"}${schedule.lastScanStatus?` · Scan ${esc(schedule.lastScanStatus)}`:""}</small></div><button class="secondary-action" data-toggle-schedule='${data(schedule)}'>${schedule.enabled?"Deaktivieren":"Aktivieren"}</button><button class="secondary-action" data-delete-schedule="${schedule.id}" data-policy-revision="${schedule.revision}">Löschen</button></article>`).join("")||'<p class="admin-copy">Keine Scan-Zeitpläne vorhanden.</p>'}</div></section>
    <section class="admin-section"><div class="section-heading"><div><p class="eyebrow">AUFBEWAHRUNG</p><h2>Bereinigungsvorschau</h2></div><span>${retention.artifactBytes.toLocaleString()} Byte</span></div><p class="admin-copy">${retention.artifactRuns} Artefaktläufe, ${retention.artifactFiles} Dateien und ${retention.diagnostics} Diagnoseeinträge wären betroffen. Originale und Katalogdaten bleiben erhalten.</p><button id="run-retention" class="secondary-action">Sichere Bereinigung ausführen</button></section>`;
}

export function bindPolicyActions(host:HTMLElement,refresh:()=>Promise<void>):void{
  host.querySelector<HTMLFormElement>("#create-profile")?.addEventListener("submit",event=>{event.preventDefault();const form=event.currentTarget as HTMLFormElement;const values=new FormData(form);void act(host,async()=>{const confirmation=await confirmExact("APPLY PROFILE new","Das Profil beeinflusst zukünftige Konvertierungen; bestehende Snapshots bleiben unverändert.");await policyApi.createProfile({name:String(values.get("name")),nativePreset:String(values.get("nativePreset")) as "web"|"fine",linearDeflection:numberOrNull(values.get("linearDeflection")),angularDeflection:numberOrNull(values.get("angularDeflection")),enabled:true},confirmation);await refresh();});});
  host.querySelectorAll<HTMLButtonElement>("[data-toggle-profile]").forEach(button=>button.addEventListener("click",()=>void act(host,async()=>{const profile=JSON.parse(button.dataset.toggleProfile!) as ConversionProfile;const confirmation=await confirmExact(`APPLY PROFILE ${profile.id}`,"Die Änderung gilt nur für künftig eingereihte Konvertierungen.");await policyApi.updateProfile(profile,{name:profile.name,nativePreset:profile.nativePreset,linearDeflection:profile.linearDeflection,angularDeflection:profile.angularDeflection,enabled:!profile.enabled},confirmation);await refresh();})));
  host.querySelectorAll<HTMLButtonElement>("[data-retire-profile]").forEach(button=>button.addEventListener("click",()=>void act(host,async()=>{const id=button.dataset.retireProfile!;const revision=Number(button.dataset.policyRevision);await confirmExact(`RETIRE ${id}`,"Das Profil wird für zukünftige Konvertierungen stillgelegt; bestehende Snapshots bleiben erhalten.");await policyApi.retireProfile(id,revision,`RETIRE ${id}`);await refresh();})));
  host.querySelector<HTMLFormElement>("#create-schedule")?.addEventListener("submit",event=>{event.preventDefault();const form=event.currentTarget as HTMLFormElement;const values=new FormData(form);void act(host,async()=>{const confirmation=await confirmExact("APPLY SCHEDULE new","Der aktive Zeitplan kann automatisch Bibliotheksscans einreihen.");await policyApi.createSchedule({libraryId:String(values.get("libraryId")),name:String(values.get("name")),localTime:String(values.get("localTime")),timeZone:String(values.get("timeZone")),weekdayMask:Number(values.get("weekdayMask")),fullScan:values.has("fullScan"),enabled:true},confirmation);await refresh();});});
  host.querySelectorAll<HTMLButtonElement>("[data-toggle-schedule]").forEach(button=>button.addEventListener("click",()=>void act(host,async()=>{const schedule=JSON.parse(button.dataset.toggleSchedule!) as ScanSchedule;const confirmation=await confirmExact(`APPLY SCHEDULE ${schedule.id}`,"Die Änderung steuert künftige automatische Scanläufe.");await policyApi.updateSchedule(schedule,{libraryId:schedule.libraryId,name:schedule.name,localTime:schedule.localTime,timeZone:schedule.timeZone,weekdayMask:schedule.weekdayMask,fullScan:schedule.fullScan,enabled:!schedule.enabled},confirmation);await refresh();})));
  host.querySelectorAll<HTMLButtonElement>("[data-delete-schedule]").forEach(button=>button.addEventListener("click",()=>void act(host,async()=>{const id=button.dataset.deleteSchedule!;const revision=Number(button.dataset.policyRevision);await confirmExact(`DELETE SCHEDULE ${id}`,"Der Zeitplan wird gelöscht. Bereits eingereihte Scans bleiben erhalten.");await policyApi.deleteSchedule(id,revision,`DELETE SCHEDULE ${id}`);await refresh();})));
  host.querySelector<HTMLButtonElement>("#run-retention")?.addEventListener("click",()=>void act(host,async()=>{await confirmExact("PURGE DERIVED ARTIFACTS","Abgeleitete Artefakte und Diagnoseeinträge werden gemäß Aufbewahrung bereinigt. Originale und Katalogdaten bleiben erhalten.");await policyApi.runRetention("PURGE DERIVED ARTIFACTS");await refresh();}));
}
function numberOrNull(value:FormDataEntryValue|null):number|null{const text=String(value??"").trim();return text===""?null:Number(text);}
function data(value:unknown):string{return esc(JSON.stringify(value));}
function esc(value:string):string{return value.replaceAll("&","&amp;").replaceAll("<","&lt;").replaceAll(">","&gt;").replaceAll('"',"&quot;").replaceAll("'","&#39;");}

async function act(host: HTMLElement, action: () => Promise<void>): Promise<void> {
  if (host.dataset.policyPending === "true") return;
  host.dataset.policyPending = "true";
  let message = host.querySelector<HTMLElement>("#admin-message, [data-policy-message]");
  if (!message) {
    message = document.createElement("p");
    message.dataset.policyMessage = "";
    message.setAttribute("role", "status");
    message.setAttribute("aria-live", "polite");
    host.prepend(message);
  }
  message.textContent = "";
  message.className = "admin-message";
  try {
    await action();
  } catch (error) {
    message.className = "admin-message error";
    message.textContent = error instanceof Error ? error.message : "Vorgang fehlgeschlagen";
  } finally {
    delete host.dataset.policyPending;
  }
}
