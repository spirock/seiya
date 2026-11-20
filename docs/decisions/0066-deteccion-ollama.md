# ADR0066 · detección inicial de archivos Ollama

El adaptador portable usa los campos de ruta existentes, sin editar assets:
llm_binary preconfigura un candidato por SO y permite override persistente.
Se amplía local-model-roles.json v1 con ollama_binary/model_store opcionales;
los archivos previos siguen legibles. Guardar metadatos sin catálogo es válido,
pero supports/start siguen exigiendo catálogo y asignaciones válidos. No se
modifican perfiles nativos MLX existentes ni se autoarranca/adopta Ollama.
El protocolo interno permite revisión sin cerebro solo al adaptador portable;
runtime_available indica disponibilidad del adaptador de configuración. La UI
existente requiere ese flag para revisar/guardar; executable sigue false sin
catálogo. Los demás adaptadores conservan el requisito de cerebro no vacío.

Alternativa descartada en la detección automática: ejecutar --version o consultar
11434 durante apertura, por efectos/propiedad del runtime y porque no acredita
compatibilidad de modelos. La posterior interfaz explícita del gestor permite al
usuario actualizar inventario, descargar o borrar bajo demanda; la pantalla de
apertura sigue sin consultar el servicio.
La comprobación sigue enlaces de instalación habituales y comprueba archivo
accesible (y bits de ejecución Unix), no autenticidad ni inferencia. Un override
inválido se conserva y avisa; no fallback silencioso. Borrar el campo restaura
autodetección. No explorar discos, almacenes de modelos ni PATH arbitrario.
El aviso y la ruta se presentan al abrir/revisar Configuración. La búsqueda de
archivos se repite al obtener snapshot; no se afirma ausencia de instalación.

El perfil Linux usa un gestor temporal en 127.0.0.1:49402 para inventario y
descargas explícitas, y otra instancia en 127.0.0.1:49401 para chat solo después de
Confirmar e iniciar. Todos esos procesos usan el almacén privado de Seiya; no
adoptan ni modifican el almacén ni detienen el servicio 11434. Su inventario de
arranque tampoco ejecuta inferencia; el primer mensaje del usuario es el primer
punto de generación. Se supervisa un grupo de procesos propio para limpiar
también los runners hijos al detener el host. Ver `BACKENDS_LOCALES.md`.
