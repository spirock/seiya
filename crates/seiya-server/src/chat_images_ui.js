// Explicit chat-image increment. Reuse the frozen composer and image panel.
// No model/runtime calls on load, no replay after reload, no untrusted HTML.
const seiyaChatWithoutImages=seiyaChatFetch;
$('deleteChat').addEventListener('click',event=>{
 if(activeChat()?.chatImagePending){event.preventDefault();event.stopImmediatePropagation();$('status').textContent='Chat conservado: hay un envío sin confirmar. Consulta el panel y recupera el mismo mensaje antes de reenviar.';}
},true);
seiyaChatFetch=async function(headers,payload){
 const chat=activeChat();if(!chat||typeof chat.id!=='string')throw Error('Conversación no disponible; no se envió nada.');
 const text=payload.messages?.at(-1)?.content;
 if(typeof text!=='string')throw Error('Mensaje inválido; no se envió nada.');
 let pending=chat.chatImagePending;
 if(pending&&(typeof pending.id!=='string'||!seiyaUuid(pending.id)||pending.text!==text))throw Error('Hay un envío de chat/imagen sin confirmar. Consulta el panel y conserva el mensaje antes de reenviar.');
 const session=await seiyaImageSession(chat.id);
 if(!pending){pending={id:crypto.randomUUID(),text};chat.chatImagePending=pending;if(!saveChats()){delete chat.chatImagePending;throw Error('No se pudo guardar la referencia; no se envió el chat.');}}
 const response=await seiyaChatWithoutImages(headers,{...payload,image_session:session,image_request_id:pending.id});
 const data=await response.json();
 if(data.image_job&&(!seiyaUuid(data.image_job.job_id)||data.image_job.request_id!==pending.id||!Object.hasOwn(seiyaImageLabels,data.image_job.phase)))throw Error('Registro de imagen inválido. Consulta el panel antes de reenviar.');
 delete chat.chatImagePending;saveChats();
 if(data.image_job)await seiyaRefreshImages();
 return {ok:true,json:async()=>data};
};
