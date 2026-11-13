# ADR0033 · flujo web opt-in y pruebas sin modelo

S4-07 en desarrollo, no servicio activado. Reutilizar búsqueda existente y
codec0.1-draft.1; no crear tools implícitas en /v1 ni ejecutar las de OpenCode.
Coordinación Python mínima, política/extracción Rust. Un worker y un slot del
coordinador por run, sin tareas auxiliares, modelos paralelos o retries ocultos.

La primera integración es una función de librería con búsqueda/lector/admisión
de contexto inyectados. No aceptar un cliente lector o presupuesto en el JSON
del navegador. Sin callback de presupuesto del host no se permite inferencia:
fixtures usan presupuesto sintético explícito; no se afirma tokenización real.

El modelo puede proponer búsquedas (hasta3) o IDs (hasta3 únicos) de resultados
del run. No URL arbitraria, enlace de páginas o resultado de otro run. Un batch
de lectura por run, tiempo restante45s compartido sin rearmar; el adaptador de
búsqueda debe obedecer ese deadline (el buscador existente aún requiere adaptación).
Texto/sources son datos no confiables enviados como resultados tool, nunca como
instrucciones de sistema. El host valida IDs/citas y enlaces entregados al modelo;
una cita válida no demuestra soporte semántico. Fuentes no leídas no son citas
de artículo ni prueba de noticias del día. No aceptar URL inventada por el modelo.

El flujo termina con contenido y citas estructuradas de fuente por ID, con errores
parciales visibles. Respuesta solo con enlaces es no resuelta. Las10 tareas de
síntesis del workload deben probarse con fixtures; eso prueba orquestación, no
calidad Gemma (S4-09). UI, cancelación, cliente HTTP autenticado y presupuesto
de tokens real siguen pendientes hasta sus pruebas; no cambiar historial o perfil
activo para simular una integración terminada. Rollback: ruta antigua de snippets.

## Integración S4-07

El perfil Python `web_reader` separado exige fast/un worker, secreto de frontend
y secreto de lector desde variables de entorno, contexto candidato explícito y
reserva de salida/margen. No edita el perfil activo. Admisión estimada conservadora
basada en bytes del JSON completo más overhead por mensaje/tool, salida y margen;
no se presenta como tokenizador, garantía universal o perfil medido. El operador
debe validar sus límites efectivos antes de pruebas reales de S4-09.

Se añade selección autenticada `POST /internal/reader/v0.1/select` en el router
de librería: frame original+respuesta correlacionada+IDs. Ejecuta la función Rust
existente, sin red ni nueva lectura. Frame≤160KiB, respuesta≤128KiB, y caps de
selección8000/16000. No aceptar texto que no provenga del resultado validado.
El endpoint no acredita procedencia de búsqueda: esa lista vive en el host Python.

Frontend opt-in: crear run mediante POST autenticado, ID/token opacos efímeros
generados por servidor; ejecutar `/api/chat` con esa referencia, consultar estado
y cancelar con referencia antes/después del upload. No usar IDs de conversación
como permisos. Registro acotado y TTL, no reanudar/repetir un run ya consumido.
Cancelar durante inferencia no libera el slot: esperar respuesta/plazo del runtime
y suprimir éxito tardío. El lector recibe cancel explícito si hubo lectura;202
no confirma cleanup. Refresh conserva historial pero no reenvía ni reanuda runs.
La API `/v1` permanece sin herramientas web implícitas ni tokens de runs browser.

Estado final S4-07: integración descrita implementada, con select Rust, perfil
Python/registro/UI y pruebas en loopback efímero con transporte/modelo falsos.
El contrato y los adaptadores inyectados de la primera sección se conservan;
las menciones de integración pendiente allí son históricas. No CLI del lector
Rust, perfil activo modificado o validación real de Gemma/Safari/páginas. La
estimación de bytes es explícita y no garantiza contexto; S4-09 debe verificar
perfil/tokenizador/runtime real antes de uso operativo. Detalles en la guía
`docs/LECTOR_WEB_EXPERIMENTAL.md` y evidencia del sprint.
