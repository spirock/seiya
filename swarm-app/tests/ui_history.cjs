const fs = require('node:fs');
const vm = require('node:vm');
const assert = require('node:assert/strict');
const { randomUUID } = require('node:crypto');
const script = fs.readFileSync('seiya/ui.html', 'utf8').split('<script>')[1].split('</script>')[0];

class Element {
  constructor() { this.value = ''; this.children = []; this.attrs = {}; }
  append(...children) { this.children.push(...children); for (const c of children) if (typeof c === 'object') c.parent = this; }
  setAttribute(k, v) { this.attrs[k] = v; }
  remove() { if (this.parent) this.parent.children = this.parent.children.filter(c => c !== this); }
  focus() {}
  replaceChildren() { this.children = []; }
}

const stored = new Map();
function page(failStorage = false, reader = false) {
  const els = {};
  const get = id => els[id] ??= new Element();
  let resolve;
  const calls = [], timers = [];
  const ctx = {
    document: { getElementById: get, createElement: () => new Element(), createTextNode: text => ({ text }), querySelectorAll: () => [] },
    crypto: { randomUUID }, Date, URL, confirm: () => true,
    localStorage: {
      getItem: key => stored.get(key) ?? null,
      setItem: (key, value) => { if (failStorage) throw Error('full'); stored.set(key, value); },
    },
    setInterval: fn => {timers.push(fn);return timers.length;}, clearInterval: () => {},
    fetch: (path, options) => {calls.push({path,options});if(path==='/health')return Promise.resolve({ok:true,json:async()=>({status:'ok',web_reader:reader?{enabled:true}:undefined})});if(path==='/api/reader/runs')return Promise.resolve({ok:true,json:async()=>({run_id:'SYNTHETIC_RUN',token:'NEVER_STORE_RUN_TOKEN'})});if(path==='/api/reader/cancel')return Promise.resolve({ok:true,json:async()=>({status:'cancellation_requested'})});if(path==='/api/reader/status')return Promise.resolve({ok:true,json:async()=>({phase:'reading'})});return new Promise(r=>{resolve=r;});},
  };
  vm.createContext(ctx);
  vm.runInContext(script, ctx);
  return { get, ctx, calls, timers, run: code => vm.runInContext(code, ctx), respond: response => resolve(response) };
}

(async () => {
  let p = page();
  const id = p.run('activeId');
  assert.equal(p.get('chatList').children.length, 1);
  assert.equal(p.get('chatList').children[0].attrs['aria-current'], 'true');
  p.get('prompt').value = 'Hola';
  p.get('mode').value = 'fast';
  p.get('key').value = 'NEVER_STORE_THIS';
  const pending = p.get('form').onsubmit({ preventDefault() {} });
  assert.equal(p.get('prompt').value, '');
  assert.equal(p.get('history').attrs['aria-busy'], 'true');
  assert.equal(p.get('chatList').children[0].disabled, true);
  assert.equal(p.get('chatList').children[0].textContent, 'Hola');
  // Refresh while generation is pending: the submitted message is not lost.
  let refreshed = page();
  assert.equal(refreshed.run('activeId'), id);
  assert.equal(refreshed.run('messages[0].content'), 'Hola');
  assert.match(refreshed.get('status').textContent, /sin respuesta/);
  p.get('prompt').value = 'Un borrador';
  p.get('prompt').oninput();
  p.respond({ ok: true, json: async () => ({ choices: [{ message: { content: 'Hola también' } }], swarm: { elapsed_seconds: 1, mode: 'fast' }, web: { sources: [{ title: 'Fuente', url: 'https://example.org' }] } }) });
  await pending;
  assert.equal(p.get('history').attrs['aria-busy'], 'false');
  assert.equal(p.get('chatList').children[0].disabled, false);
  assert.equal(p.get('prompt').value, 'Un borrador');
  assert.ok(!stored.get('seiya.chats.v1').includes('NEVER_STORE_THIS'));
  p = page();
  assert.equal(p.run('activeId'), id);
  assert.equal(p.run('messages.length'), 2);
  assert.equal(p.get('prompt').value, 'Un borrador');
  assert.equal(p.run('activeChat().sources[1].sources[0].title'), 'Fuente');
  p.get('clear').onclick();
  assert.notEqual(p.run('activeId'), id);
  assert.equal(p.run('chats.length'), 2);
  assert.equal(p.get('chatList').children.length, 2);
  p.get('chatList').children[1].onclick();
  assert.equal(p.run('activeId'), id);
  assert.equal(p.get('chatList').children[1].attrs['aria-current'], 'true');
  assert.equal(p.get('chatList').children[0].attrs['aria-current'], 'false');
  p.get('chats').value = id;
  p.get('chats').onchange();
  assert.equal(p.run('messages.length'), 2);
  p.get('deleteChat').onclick();
  assert.equal(p.run('chats.length'), 1);
  assert.equal(p.get('chatList').children.length, 1);
  assert.notEqual(page().run('activeId'), id);
  const broken = page(true);
  assert.match(broken.get('storageStatus').textContent, /No se pudo guardar/);
  const original = '{corrupted'; stored.set('seiya.chats.v1', original);
  page();
  assert.equal(stored.get('seiya.chats.v1'), original);
  assert.match(p.run('runtimeLabel({})'), /Runtime: no comprobado/);
  assert.match(p.run('runtimeLabel({runtime_status:{endpoint_observation:"connection_not_established",admission:"blocked",memory_admission:"not_integrated"}})'), /conexión no establecida.*residencia desconocida.*sin supervisión integrada.*bloqueadas/);
  assert.match(p.run('runtimeLabel({runtime_status:{endpoint_observation:"last_request_completed",admission:"may_attempt",memory_admission:{state:"monitoring"}}})'), /última petición finalizada.*no garantiza capacidad/);
  assert.match(p.run('runtimeError({error:{code:"runtime_state_unconfirmed"}},"fallback")'), /No se conoce si terminó/);
  const failedPage=page();failedPage.get('prompt').value='Conservar este mensaje';
  const failure=failedPage.get('form').onsubmit({preventDefault(){}});
  failedPage.respond({ok:false,json:async()=>({error:{code:'runtime_unavailable',message:'raw backend detail'}})});
  await failure;
  assert.match(failedPage.get('status').textContent,/Runtime no disponible/);
  assert.equal(failedPage.get('prompt').value,'Conservar este mensaje');
  assert.equal(failedPage.run('messages.length'),0);
  stored.clear();
  const readerPage=page(false,true);
  await readerPage.run('refreshRuntimeStatus()');
  readerPage.get('prompt').value='Resume un artículo';
  const readerPending=readerPage.get('form').onsubmit({preventDefault(){}});
  for(let i=0;i<8;i++)await Promise.resolve();
  assert.equal(readerPage.calls.filter(c=>c.path==='/api/chat').length,1);
  assert.equal(readerPage.run('readerCancel.hidden'),false);
  const chatCall=readerPage.calls.find(c=>c.path==='/api/chat');
  assert.equal(JSON.parse(chatCall.options.body).reader_run.token,'NEVER_STORE_RUN_TOKEN');
  assert.ok(!stored.get('seiya.chats.v1').includes('NEVER_STORE_RUN_TOKEN'));
  await readerPage.run('readerCancel.onclick()');
  assert.match(readerPage.get('status').textContent,/no se confirma limpieza/);
  assert.equal(readerPage.run('busy'),true);
  readerPage.respond({ok:false,json:async()=>({error:{message:'reader_cancelled: cancellation requested'}})});
  await readerPending;
  assert.equal(readerPage.get('prompt').value,'Resume un artículo');
  assert.equal(readerPage.run('readerCancel.hidden'),true);
  const refreshedReader=page(false,true);
  assert.equal(refreshedReader.calls.filter(c=>c.path==='/api/chat').length,0);
  readerPage.run('sources({reads:[{id:"s1",status:"ok",text_truncated:true}],sources:[{id:"s1",title:"<img onerror=evil>",url:"https://example.org"}]})');
  const boxes=readerPage.get('history').children;
  assert.match(boxes.at(-2).textContent,/extracto recortado/);
  assert.match(boxes.at(-1).children[0].textContent,/<img onerror=evil>/);
  assert.ok(!script.includes('innerHTML'));
  console.log('UI history: reload, IDs, drafts, sources, in-flight recovery, switching, deletion and storage failures OK');
})().catch(error => { console.error(error); process.exitCode = 1; });
