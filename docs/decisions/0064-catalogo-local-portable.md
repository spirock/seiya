# ADR0064 · catálogo sustituible y backend HTTP local portable

Estado: implementación de texto con pruebas sintéticas; validación de dispositivo
y motores reales pendiente. No incluye adaptador portable de imágenes.

## Decisión y límites

Se conserva HTML/CSS/JavaScript y su diseño. No portar esa interfaz a Rust ni
modificar sus assets en este incremento; contrato/hash de assets como regresión.
El backend puede proporcionar datos/instrucciones efectivos por los protocolos
existentes sin sustituir frontend ni anunciar funciones no implementadas.

Se añade un catálogo versionado de modelos de texto con ID local, binding real,
endpoint OpenAI local, capacidades declaradas y límites explícitos. Asignaciones
cerebro y hasta16 trabajadores únicos se validan contra el catálogo, sin marcas
obligatorias. Cambios solo tras revisión/confirmación y con el host anterior
detenido; snapshot inmutable por host/run. Sin fallback de modelo o remoto.

Alternativas: fijar nuevos tags perpetúa el problema; portar kernels de inferencia
no aporta compatibilidad automática; ejecutar binarios descargados requiere otro
contrato de proceso/sandbox. Se elige conectar motores externos ya preparados,
por ejemplo llama.cpp con API OpenAI, sin iniciarlos, adoptar su PID, descargarlos
o detenerlos. Esto es un backend portable, no un runtime de inferencia en Rust.

Solo HTTP canónico127.0.0.1 con puerto explícito y /v1, sin DNS/proxy/redirecciones
o reintentos. Descubrimiento de binding por /v1/models antes del POST; no inferencia
al construir/guardar/iniciar web. Capacidad declarada no equivale a calidad o
compatibilidad del tokenizador/modelo/dispositivo probadas. Las imágenes conservan
el adaptador pinned macOS; no anunciar ejecución de imagen portable.

## Propiedad y recursos

Un turno global por host, selección secuencial del pool y locks cooperativos por
puerto en autoridad fija por usuario. En macOS también respetar lease/cuarentena
MLX existente. No garantiza exclusión frente a clientes externos no cooperativos:
el operador debe dedicar los endpoints a esta instancia durante el ensayo.
No se asume que todos los modelos instalados sean residentes ni que liberar un
slot HTTP descargue pesos. RAM/VRAM y carga/descarga siguen a cargo del motor.

Observaciones desconocidas o fuera de política bloquean nuevos envíos. Lecturas
locales de memoria del SO, con fixtures en tests; no inventar RAM segura por marca
de modelo. Si empeoran durante un turno, cerrar admisión y esperar respuesta o
deadline. Cancelar HTTP no prueba que el motor haya parado: nunca liberar el
slot como si cleanup estuviera confirmado por cerrar un socket.

Journal durable de intención ANTES del POST. Error ambiguo, cancelación/drop,
timeout o caída conservan journal y bloquean el endpoint en próximos arranques;
sin borrado/recuperación automática. Respuesta válida y binding correcto permiten
cerrar el journal propio. No matar procesos ajenos ni enviar comandos de unload
no contratados. Retención máxima de contexto acotada, sin truncamiento silencioso.

## Migración y entrega

Catálogo local separado de desktop.json/desktop-manager.json; no reescribirlos.
Roles portables en documento propio, con CAS respecto al catálogo y asignaciones.
La web existente usa sus campos de roles; las rutas de Ollama/MLX quedan vacías
en este adaptador HTTP y se explica por datos del backend que no se usan.
Selección de adaptador por catálogo explícito o catálogo fijo del equipo; abrir
no autoriza inferencia. Host macOS pinned y --serve anteriores siguen como rollback.

Configuración, conectores y recursos con pruebas sintéticas HTTP/OS/filesystem.
Sin instalar motores, descargar pesos, inferencia real, publicación o cambios de
OpenCode. Builds/GUI Windows y Linux reales se registran aparte, no se deducen de
compilar en macOS. La API OpenCode existente mantiene ID público pegassus y tools
ejecutadas por OpenCode, no duplicadas en Seiya.
