# Catálogo de texto local v1 · experimental

Ejemplo: `catalog.example.json` en las fuentes y `local-models.example.json` en el
paquete, con bindings ficticios: **no ejecutarlo tal cual**.
Sustituir IDs/bindings/puertos por los de motores propios ya preparados. Ninguna
marca es obligatoria. Importar un modelo al catálogo no instala pesos, valida sus
tools o acredita memoria suficiente. Imagen HTTP opt-in disponible con validación
PNG nativa macOS; visión/audio ausentes. El pipeline macOS anterior conserva otra
entrada y sus pins. Ver [imágenes](../../../docs/IMAGENES_DESDE_CHAT.md) y el
[ejemplo separado](images.example.json), no ejecutable sin motores preparados.

JSON estricto≤65536bytes, versión1, 1–32 modelos registrados. Campos:

- models: id local único (1–128 ASCII alfanuméricos/guion/underscore), runtime,
  supports_tools (declaración, no certificación), max_input_bytes (1024–131072).
- runtime: contrato RuntimeConfig existente, backend=openai_local, id igual al
  ID del registro y model=binding exacto publicado por el motor; presupuesto de
  salida1–4096 y timeout1–600s. Sin API keys incrustadas, URL remota, DNS, proxy,
  redirects o retries. Solo `http://127.0.0.1:PUERTO/v1` canónico. Se rechazan
  puertos8015/8016/11434/49399/49400 para no adoptar hosts/servicios anteriores.
- assignments: brain registrado y workers=[] o hasta16 modelos registrados,
  únicos y distintos del cerebro. Con workers se requiere supports_tools=true
  para el cerebro. Se despacha un trabajador por turno, round-robin; no votación
  ni clasificación semántica de especialidades, ni todos residentes a la vez.
- minimum_available_mib:256–1048576; reserva de admisión Windows/Linux, no tamaño
  de los pesos ni promesa de RAM/VRAM suficiente. macOS usa presión nativa normal
  y lease/cuarentena del adaptador existente, no esta cifra como RAM medida.
- web_search: boolean opcional, false por defecto. true exige cerebro con
  supports_tools=true y max_input_bytes≥8192. Solo chat web; no /v1. Proveedor
  fijo HTTPS html.duckduckgo.com, sin redirects de búsqueda, proxy ni fallback.
  La revisión informa de la transferencia de consultas. No busca al abrir/iniciar.
  Activar en una copia configurada: `"web_search": true`. El ejemplo mantiene
  opt-in desactivado; declarar tools no prueba que el modelo las produzca bien.
- image_models: opcional[], hasta16 registros {id,runtime}. IDs únicos frente a
  texto e imagen; RuntimeConfig local estricto con backend=openai_local. Su
  max_output_tokens no se usa al generar PNG. Mismos puertos reservados/locks.
- image: ID de image_models o null (por defecto), nunca una ruta/peso/URL.
- image_chat: boolean opcional false; true requiere image seleccionado y cerebro
  con tools e input≥8192bytes. Herramienta image_generate solo con prompt; host
  fija modelo,512×512 y semilla42. Una sola primera acción antes de datos web.
  Registro de trabajo no equivale a PNG completado. GET /v1/models + POST
  /v1/images/generations; respuesta model exacto y una entrada data[].b64_json, sin URLs.

`--check-catalog FILE` valida offline estructura/rutas de endpoint, no se conecta
ni comprueba pesos. El archivo regular/canónico no puede ser symlink; en Unix
debe ser privado. Catálogo fijo: `local-models.json` bajo la carpeta de Seiya:

- macOS: `~/Library/Application Support/Seiya/`.
- Linux: `~/.local/state/Seiya/` (no mover automáticamente desde otras rutas).
- Windows: `%LOCALAPPDATA%\Seiya\`.

`seiya-desktop --launch --catalog RUTA_ABSOLUTA` permite elegir otro archivo sin
copiarlo o modificarlo. La autoridad de locks/roles permanece fija por usuario;
cambiar el catálogo no evade journals. Sin catálogo fijo, macOS conserva su perfil
MLX anterior; Windows/Linux muestran cómo preparar el catálogo, sin fallback.

## Protocolo del motor y propiedad

Motor externo ya preparado, por ejemplo llama.cpp con API OpenAI. No se inicia,
instala, adopta PID ni detiene ese motor. Debe dedicarse a Seiya durante la sesión;
otros clientes no cooperativos podrían usarlo: estos locks no son un sandbox.
El backend verifica GET /v1/models antes de cada POST /v1/chat/completions, exige
binding exacto y comprueba el modelo devuelto. Ninguna inferencia al abrir web,
detectar, revisar, guardar o iniciar host; únicamente al solicitar un turno.

Un slot global compartido por cerebro/trabajadores, lock global por usuario y locks
persistentes por puerto. Cualquier journal pendiente bloquea también otros puertos
del mismo usuario; cambiar catálogo no permite evadirlo.
Journal durable propio antes del POST, retirado solo tras respuesta completa
aceptada. Timeout, drop/cancelación o respuesta ambigua dejan marker y bloquean
el endpoint, incluso después de reiniciar. Nunca borrar markers para reintentar:
verificar primero estado/propiedad del motor mediante intervención operativa.
Esta versión no implementa recuperación automática. Cerrar HTTP no acredita
cancelación del motor; Seiya espera respuesta/deadline y conserva incertidumbre.

Recursos: Linux MemAvailable, Windows GlobalMemoryStatusEx (disponible físico y
carga<95%), macOS presión nativa normal. Unknown/bajo mínimo bloquea; empeorar
cierra admisión y drena, no finge unload. Sin medición de VRAM/modelos completos,
no es todavía un gestor adaptativo de residencia. Requests demasiado grandes se
rechazan, nunca se truncan silenciosamente.

## Gestor sin cambiar frontend

HTML/CSS/JS existentes permanecen congelados. El backend preconfigura la ruta
Ollama por SO y comprueba su archivo; el override es editable. Ruta y almacén
son metadatos, no endpoints ni orden de ejecución. Imágenes quedan vacías.
El catálogo define los endpoints. Se usan los campos libres de cerebro,
workers e imagen con IDs registrados; imagen vacía si no se configura.
La revisión muestra endpoints/bindings como efectos del host; guardar escribe
solo `local-model-roles.json` con CAS frente a catálogo/asignaciones y rutas. Sin
catálogo permite guardar rutas, pero no iniciar. Sus campos v1 adicionales
ollama_binary/model_store son opcionales al leer archivos anteriores. Catálogo,
desktop.json y desktop-manager.json no se sobrescriben. Inicio usa el snapshot
aprobado; cambiar configuración exige detener y revisar de nuevo.
El archivo de roles admite image opcional; ausente hereda catálogo, string vacío
quita la selección (si image_chat=false). No habilita nuevas tools en perfiles
nativos anteriores ni convierte una API Ollama estándar en generador de imagen.

En Windows se usan permisos heredados de LOCALAPPDATA y flags antirreparse;
no se acredita auditoría de ACL/owner o crash-durability de directorio equivalente
a fsync Unix. No hay credenciales/conversaciones en esos documentos. Validación
real de SO/motor/GPU y empaquetado cruzado se registra aparte de fixtures.

La API OpenCode publicada conserva su contrato anterior. Un backend=openai_local
no se admite en la configuración API simple, para evitar saltarse propiedad y
journals del catálogo. Este incremento conecta el catálogo al shell/chat web,
no instala un plugin ni reconfigura OpenCode automáticamente.
