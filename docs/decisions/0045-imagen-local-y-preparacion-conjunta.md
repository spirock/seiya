# ADR0045 · imagen local y unión conjunta solo preparada

Selección acotada: variante Unsloth Turbo Q4,3 assets exactos y runtime privado
sd.cpp/Metal, una imagen sintética512×512≤600s sin LLM residentes. Para Gemma+
Qwen se limita a **preparar integración**: ningún ensayo conjunto o activación.

La lógica nueva compartida se implementa en Rust. Se retiran los prototipos Python nuevos de descarga/smoke
tras portar fronteras/tests. Descarga ya verificada se conserva, no se repite.
Supervisor/descarga/artefactos y preparación de unión nuevos en Rust; inferencia
sd.cpp/Metal externa y UI HTML/JS existentes no se reimplementan como kernels.
No sustituir el prototipo Python operativo sin compatibilidad/rollback probados.
El decoder zlib usa el adaptador nativo del SO desde Rust, con buffers/longitudes
acotados, sin instalar paquetes o un runtime Python para imágenes.

## Límite del incremento

Nuevas fronteras de imagen en seiya-contracts son librería **no herramienta
registrada**, servidor o permiso. Request de texto con prompt≤2048bytes,
dimensiones256–1024 múltiplos32 y seed u32; sin rutas, modelos, comandos o URLs
elegidos por el modelo. Adaptador del host elige los tres archivos verificados,
runtime/build, argv fijos, salida privada y deadline. Prueba real solo512×512,
seed42, un PNG, Turbo8 pasos/CFG1/Euler/preset sigmas; sin edición/mmproj.

Solo ejecución de un binario local con procedencia/fuente/licencias revisadas,
no shell o código generado. Child original, grupo propio, libproc y lease
compartida, Ollama sin LLM residentes antes de cargar. Pesos read-only mediante
sandbox heredado. Memoria informativa, pero ownership, tiempo, disco libre mínimo
2GiB y tamaño del log4MiB protegen operación. No borrar modelos para hacer sitio,
retry del host o fallback remoto. No garantías hard-real-time/OOM.

PNG≤16MiB, salida regular propia y no symlink; firma/IHDR/CRC/IEND/chunks con
límites comprobados por Rust (inspección de contenedor, **no decoder**). Adaptador
Rust/macOS valida zlib con tope del tamaño de píxeles y filtros RGB/RGBA8bit,
no interlazado/APNG. No declarar imagen completada por existir archivo/exit0.
Hash/dimensiones/decodificación/procedencia antes de ofrecer resultado. SVG/HTML,
rutas, URLs o base64 elegidos por modelo no son imágenes autorizadas. Resultado
de esta prueba es artefacto local, no API pública o integración web disponible.

## Preparación posterior de la web

Concreción W6 antes de interfaces: router de librería `prepared_web`, sin CLI,
bind o carga. Reutiliza HTML/JS existente y protocolo browser `/api/chat` y
`/api/jobs`, con historial separado `seiya.prepared.chats.v1` y estado de
integración no activada en la línea de estado existente, sin añadir layout ni
CSS. Usuario confirma conservar el diseño actual tal como está. Lógica nueva de BrowserJobs en Rust: historial textual
cerrado, herramientas de jobs fijadas por host, una acción por respuesta,
acuse/estado formateados sin otra inferencia. Sesión y UUID del envío nunca
pasan al modelo. No lector, imagen, shell o `/v1` chat en este router browser.

Un `HostGate` inyectado es snapshot rápido del host propietario: debe acreditar
permiso conjunto, lease/identidades y recursos de **todo** el par, no una boolean
en JSON o la suma de calibraciones. Default denegado, sin implementación nativa
positiva en este incremento. Scheduler foreground y Jobs comparten ese gate;
cuarentena del broker bloquea nuevas inferencias. Status/cancel/descarga permanecen
accesibles con auth incluso si se revoca admisión. Gate no cancela por sí solo una
inferencia iniciada: vigilancia/cleanup deben venir del host antes de activación.

Alternativas: conectar UI directamente a dos puertos MLX pierde ownership y
validación; duplicar un frontend Python contradice preferencia Rust; activar un
router público de tools mezcla OpenCode y permisos. Se mantiene router browser
separado de API OpenAI y prototipo operativo. El objeto host inyectado es frontera
de confianza del programa, nunca un dato recibido del modelo/navegador.
Migración opt-in: futuro host propietario inyectará scheduler/ejecutor y gate
validado tras W7; no se cambia config ni se arranca listener ahora. Rollback:
no usar constructor `prepared_web`, conservar preview y aplicación anteriores.

Reutilizar broker de trabajos de texto y UI, sin exponer nuevas tools planned.
La unión Gemma/Qwen necesita un host propietario del runtime y gate fail-closed,
schema/historial/ack de host/cancelación/errores probados con mocks. Modelos
reales y memoria de las pruebas individuales no conceden admisión del par.
El contrato de imagen no se añade silenciosamente al protocolo jobs-v1 de texto;
separar capacidades y artefactos PNG de borradores. Primera integración debe
mantener catálogo efectivo vacío para imágenes si no hay ejecutor validado.

Alternativas: reenviar a una API remota cambia proveedor y privacidad; llamar al
generador por Ollama chat confunde contratos; anunciar una conversión descargada
como disponible engaña; escribir el PNG en un proyecto/leer fotos amplía alcance.
Se eligen adaptador explícito y evidencia real separada de mocks.

Rollback: omitir nuevos módulos/entrypoints, conservar UI/demo y configs previas;
detener solo procesos propios. No instalar, publicar o redistribuir pesos/runtime
desde el paquete Seiya. Licencias de los assets restringen a evaluación no comercial;
el MIT del runtime no relicencia los pesos. Conservar descargas y evidencia.
