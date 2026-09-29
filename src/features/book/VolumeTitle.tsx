import { useEffect, useState } from "react";
import { projectApi as api } from "../../shared/api/projects";
import { Modal } from "../../shared/ui/Modal";
import { errorText, type T } from "../../app/strings";
export function VolumeTitle({projectId, source, t, onClose, onSaved}: {projectId:string; source:string; t:T; onClose:()=>void; onSaved:()=>Promise<void>}) {
  const [title,setTitle]=useState("");
  const [revision,setRevision]=useState<string|null>(null);
  const [busy,setBusy]=useState(false);
  const [error,setError]=useState<unknown>(null);
  useEffect(()=>{
    let live=true;
    api.volume({projectId,source}).then(value=>{if(live){setTitle(value.title);setRevision(value.revision);}}).catch(e=>{if(live)setError(e);});
    return ()=>{live=false;};
  },[projectId,source]);
  async function translate(){
    setBusy(true);setError(null);
    try{setTitle(await api.translateVolume({projectId,source}));}catch(e){setError(e);}finally{setBusy(false);}
  }
  async function save(){
    if(revision===null)return;
    setBusy(true);setError(null);
    try{await api.saveVolume({projectId,source,title,expectedRevision:revision});await onSaved();onClose();}catch(e){setError(e);}finally{setBusy(false);}
  }
  return <Modal title={t("volumeTitle")} closeLabel={t("close")} onClose={onClose} busy={busy} footer={<>
    <button disabled={busy || revision===null} onClick={()=>void translate()}>{t("translateTitle")}</button>
    <button disabled={busy || revision===null} onClick={()=>void save()}>{t("save")}</button>
  </>}>
    <p>{source}</p>
    <label>{t("volumeTitle")}<input value={title} disabled={busy || revision===null} onChange={e=>setTitle(e.target.value)} /></label>
    {busy && <p role="status">{t("volumeTitleWorking")}</p>}
    {error!=null && <p role="alert">{errorText(error,t)}</p>}
  </Modal>;
}
