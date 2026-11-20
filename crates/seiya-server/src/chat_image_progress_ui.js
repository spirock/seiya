// Chat-only presentation of observed broker jobs. The frozen image controls
// remain the sole owner of cancellation, PNG retrieval and idempotency.
const seiyaPanelImageRender=seiyaRenderImages;
const seiyaImageStyle=document.createElement('style');
seiyaImageStyle.textContent=`
.seiya-image-spinner {display:inline-block;width:1em;height:1em;margin-inline-end:.5em;vertical-align:-.15em;border:.15em solid currentColor;border-inline-end-color:transparent;border-radius:50%;box-sizing:border-box}
@keyframes seiya-image-turn {to {transform:rotate(1turn)}}
@media (prefers-reduced-motion:no-preference) {.seiya-image-spinner {animation:seiya-image-turn 1s linear infinite}}
`;
document.head.append(seiyaImageStyle);
const seiyaChatImageStages={queued:'Imagen en cola…',preparing:'Preparando imagen…',running:'Generando imagen…',validating:'Validando imagen…',cancelling:'Cancelando imagen…'};
seiyaRenderImages=function(data,chatId,session){
 seiyaPanelImageRender(data,chatId,session);
 if(chatId!==activeId)return;
 const history=$('history'),chat=activeChat();
 history.querySelectorAll('[data-seiya-image-progress]').forEach(card=>card.remove());
 // The existing panel is a fallback for manual or unbound historical jobs.
 // Its operational notices are not needed in the conversation.
 const heading=seiyaImagePanel.querySelector('h2');
 if(heading)heading.remove();
 const note=seiyaImagePanel.querySelector('p');
 if(note)note.remove();
 const messages=history.querySelectorAll('.message');
 let unbound=0;
 for(const job of data.jobs){
  const row=[...seiyaImagePanel.children].find(el=>el.getAttribute('data-image-job-id')===job.job_id);
  if(!row)continue;
  let index=chat?.imageJobMessages?.[job.request_id];
  if(!Number.isInteger(index)||chat.messages?.[index]?.role!=='assistant'){
   // A pre-upgrade conversation with exactly one registered image can be
   // associated with its one registration reply, without guessing among jobs.
   const replies=chat?.messages?.map((m,i)=>m.role==='assistant'&&m.content.startsWith('Solicitud de imagen registrada.')?i:-1).filter(i=>i>=0)||[];
   index=data.jobs.length===1&&replies.length===1?replies[0]:-1;
  }
  const target=messages[index];
  const status=row.querySelector('p');
  if(status)status.textContent=seiyaChatImageStages[job.phase]||seiyaImageLabels[job.phase];
  if(status&&job.phase==='running'){
   const wheel=document.createElement('span');
   wheel.className='seiya-image-spinner';
   wheel.setAttribute('aria-hidden','true');
   status.insertBefore(wheel,status.firstChild);
  }
  const id=document.createElement('small');
  id.textContent='ID de imagen: '+job.job_id;
  id.style.display='block';
  row.insertBefore(id,row.querySelector('button'));
  if(target){
   const card=document.createElement('section');
   card.setAttribute('data-seiya-image-progress','');
   card.setAttribute('aria-label','Estado de imagen');
   card.setAttribute('aria-live','polite');
   card.append(row);
   target.append(card);
  }else unbound++;
 }
 // Never hide a manual/unbound job or an error message from the broker.
 seiyaImagePanel.hidden=unbound===0;
};
