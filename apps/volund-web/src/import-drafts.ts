import { catalogApi } from "./api";
import { confirmExact } from "./confirmation";
import { formatBytes } from "./format";
import { requestTextInput } from "./text-input-dialog";
import type { ImportDraftLifecycle } from "./types";

export function importStatusLabel(status: ImportDraftLifecycle["status"]): string {
  return ({ draft:"Entwurf",uploading:"Upload unterbrochen",uploaded:"Hochgeladen",
    review_ready:"Prüfbereit",reviewed:"Geprüft",committing:"Wird übernommen",
    committed:"Abgeschlossen",failed:"Fehlgeschlagen",cancelled:"Abgebrochen",
    expired:"Abgelaufen" })[status];
}

export async function loadDraftInventory(section:HTMLElement,onResume:(draft:ImportDraftLifecycle)=>Promise<void>,
  openModel:(id:string)=>void):Promise<void>{
  try{
    const [page,storage]=await Promise.all([catalogApi.importDrafts(),catalogApi.importStorage()]);
    section.replaceChildren();
    const heading=document.createElement("div"); heading.className="section-heading";
    heading.innerHTML="<div><p class=\"eyebrow\">ENTWÜRFE UND HISTORIE</p><h2>Importvorgänge</h2></div>";
    const usage=document.createElement("span"); usage.textContent=`${formatBytes(storage.uploadedBytes)} hochgeladen · ${formatBytes(storage.reservedBytes)} reserviert`;
    heading.append(usage); section.append(heading);
    if(page.items.length===0){section.append(document.createTextNode("Noch keine Importentwürfe."));return;}
    const list=document.createElement("div"); list.className="admin-list import-draft-list";
    for(const draft of page.items)list.append(draftRow(draft)); section.append(list);
    list.addEventListener("click",event=>void actOnDraft(section,event,onResume,openModel));
  }catch(reason){section.textContent=reason instanceof Error?reason.message:"Importentwürfe konnten nicht geladen werden.";}
}

function draftRow(draft:ImportDraftLifecycle):HTMLElement{
  const row=document.createElement("article"); const text=document.createElement("div");
  const name=document.createElement("strong"); name.textContent=draft.displayName;
  const detail=document.createElement("small"); detail.textContent=`${importStatusLabel(draft.status)} · ${draft.uploadedFiles}/${draft.totalFiles} Dateien · ${formatBytes(draft.uploadedBytes)}`;
  text.append(name,detail); row.append(text); const actions=document.createElement("div"); actions.className="review-actions";
  if(draft.resultModelId)actions.append(button("Ergebnis öffnen","result",draft.id,draft.resultModelId));
  else if(!["cancelled","expired","committing"].includes(draft.status))actions.append(button("Fortsetzen","resume",draft.id));
  if(draft.canCancel){const rename=button("Umbenennen","rename",draft.id);rename.dataset.displayName=draft.displayName;actions.append(button("Abbrechen","cancel",draft.id),rename);}
  row.append(actions); return row;
}

function button(label:string,action:string,id:string,modelId?:string):HTMLButtonElement{
  const value=document.createElement("button"); value.type="button"; value.className="secondary-action";
  value.textContent=label; value.dataset.action=action; value.dataset.id=id; if(modelId)value.dataset.modelId=modelId; return value;
}

async function actOnDraft(section:HTMLElement,event:Event,onResume:(draft:ImportDraftLifecycle)=>Promise<void>,openModel:(id:string)=>void):Promise<void>{
  const target=(event.target as HTMLElement).closest<HTMLButtonElement>("button[data-action]"); if(!target)return;
  const id=target.dataset.id!; target.disabled=true;
  try{
    if(target.dataset.action==="result")openModel(target.dataset.modelId!);
    if(target.dataset.action==="resume")await onResume(await catalogApi.importDraft(id));
    if(target.dataset.action==="rename"){
      const name=await requestTextInput({title:"Importentwurf umbenennen",label:"Neuer Anzeigename",initialValue:target.dataset.displayName ?? "",submitLabel:"Namen übernehmen",maxLength:160});
      if(!name)return; await catalogApi.renameImport(id,name);
    }
    if(target.dataset.action==="cancel"){
      const exact=`CANCEL IMPORT ${id}`;
      await confirmExact(exact,"Der Importentwurf und seine noch nicht veröffentlichten Uploaddaten werden abgebrochen und zur Bereinigung freigegeben.");
      await catalogApi.cancelImport(id);
    }
    if(target.dataset.action!=="result")await loadDraftInventory(section,onResume,openModel);
  }catch(reason){if(reason instanceof Error&&reason.message==="Aktion abgebrochen.")return;section.textContent=reason instanceof Error?reason.message:"Entwurfsaktion fehlgeschlagen.";}
  finally{target.disabled=false;if(target.isConnected)target.focus();}
}
