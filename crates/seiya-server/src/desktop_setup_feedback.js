'use strict';
(() => {
 const form=document.getElementById('setupForm'),review=document.getElementById('setupReview');
 const original=document.getElementById('setupMessage'),approval=document.getElementById('setupApproval');
 if(!form||!review||!original||!approval)return;
 const feedback=document.createElement('p');feedback.id='setupReviewFeedback';feedback.setAttribute('role','status');feedback.setAttribute('aria-live','polite');
 review.insertAdjacentElement('afterend',feedback);
 let requested=false;
 function explain(text){
  if(/images\.binary:(absolute_path_required|existing_path_required|regular_file_required|unreadable)/.test(text))return 'Selecciona el ejecutable compatible de imágenes (sd-cli), o desactiva las imágenes en este borrador para probar solo texto. '+text;
  if(/images\.assets_dir:/.test(text))return 'Selecciona la carpeta de assets de imagen compatible, o desactiva las imágenes en este borrador. '+text;
  if(/llm_binary:/.test(text))return 'Revisa la ruta del ejecutable Ollama compatible en «Motor y almacén local». Encontrarlo no certifica sus modelos. '+text;
  if(/model_store:/.test(text))return 'Revisa la carpeta del almacén de modelos compatible en «Motor y almacén local». Instalar Seiya no instala pesos. '+text;
  return text;
 }
 function visible(node){node.scrollIntoView?.({block:'nearest'});}
 function update(){
  feedback.textContent=explain(original.textContent||'');
  if(requested){requested=false;visible(approval.hidden?feedback:approval);}
 }
 new MutationObserver(update).observe(original,{childList:true,subtree:true,characterData:true});
 form.addEventListener('submit',()=>{requested=true;feedback.textContent='Revisando configuración; espera el resultado. No se están cargando modelos.';},true);
 form.addEventListener('invalid',event=>{
  requested=false;const field=event.target;
  feedback.textContent=field?.id==='setupBrain'?'Completa el campo «Cerebro» antes de revisar.':'Completa el campo obligatorio indicado antes de revisar.';
  visible(field);field?.focus?.();
 },true);
 const without=document.createElement('button');without.id='setupWithoutImages';without.type='button';without.textContent='Desactivar imágenes en este borrador';without.disabled=review.disabled;
 review.insertAdjacentElement('afterend',without);
 without.addEventListener('click',()=>{
  for(const id of ['setupImage','setupImageBinary','setupImageAssets']){const field=document.getElementById(id);if(field)field.value='';}
  form.dispatchEvent(new Event('input',{bubbles:true}));
  original.textContent='Imágenes desactivadas solo en el borrador. No se ha guardado ni iniciado nada. Revisa motor y almacén y pulsa «Revisar configuración».';
  visible(feedback);
 });
 update();
})();
