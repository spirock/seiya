// Initial authenticated shell only. No chat/history loading or runtime actions.
const seiyaSetupNavigation=(()=>{
 const get=id=>document.getElementById(id),scroll=get('scroll'),setup=get('setup');
 const surface=document.createElement('div');
 for(const child of [...scroll.children])surface.append(child);
 const root=document.createElement('button');root.type='button';root.textContent='Configuración';
 const menu=document.createElement('nav');menu.id='seiyaInitialSettingsMenu';menu.hidden=true;menu.setAttribute('aria-label','Opciones de configuración');
 root.setAttribute('aria-controls',menu.id);root.setAttribute('aria-expanded','false');
 (document.querySelector('aside')||scroll).append(root,menu);
 const general=document.createElement('section');general.setAttribute('aria-label','Configuración general');
 const title=document.createElement('h2');title.textContent='Configuración general';
 const info=document.createElement('p');info.textContent='Los controles de diagnóstico y límites estarán disponibles cuando el host esté preparado. No se ha iniciado ningún modelo.';general.append(title,info);
 const back=document.createElement('button');back.type='button';back.textContent='Volver al chat';
 scroll.append(back,surface,setup,general);
 const composer=get('form').parentNode;const composerHidden=composer.hidden;
 let position=scroll.scrollTop,view='chat';
 function expanded(value){menu.hidden=!value;root.setAttribute('aria-expanded',String(value));}
 function show(next){
  if(view==='chat'&&next!=='chat')position=scroll.scrollTop;
  view=next;surface.hidden=next!=='chat';setup.hidden=next!=='models';general.hidden=next!=='general';back.hidden=next==='chat';
  composer.hidden=next==='chat'?composerHidden:true;
  for(const [key,button]of choices)button.setAttribute('aria-pressed',String(next===key));
  if(next==='chat'){expanded(false);scroll.scrollTop=position;}else{expanded(true);scroll.scrollTop=0;const panel=next==='models'?setup:general;panel.setAttribute('tabindex','-1');panel.focus();}
 }
 const choices=new Map();
 for(const [key,label]of [['models','Configuración de modelos'],['general','Configuración general']]){
  const button=document.createElement('button');button.type='button';button.textContent=label;button.style.display='block';button.onclick=()=>show(key);choices.set(key,button);menu.append(button);
 }
 root.onclick=()=>expanded(menu.hidden);back.onclick=()=>{show('chat');root.focus();};
 for(const panel of [menu,setup,general,back])panel.addEventListener('keydown',event=>{if(event.key==='Escape'){event.preventDefault();show('chat');root.focus();}});
 for(const id of ['clear','newMobile'])get(id).addEventListener('click',()=>show('chat'));
 get('welcome').textContent='Abre Configuración → Configuración de modelos para revisar este equipo. Guardar no inicia modelos ni habilita por sí solo el chat.';
 show('chat');
 return {show};
})();
function seiyaSetupSaved(){
 seiyaSetupNavigation.show('chat');
 document.getElementById('runtimeStatus').textContent='Configuración guardada sin iniciar. El chat requiere un host preparado; guardar no recupera un runtime bloqueado.';
 document.getElementById('status').textContent='Configuración guardada sin iniciar motores.';
}
