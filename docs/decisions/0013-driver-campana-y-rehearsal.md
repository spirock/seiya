# ADR 0013 · integrar preparación y driver sin probar MLX por accidente

S3-04b, 2026-10-19. Se reutiliza el lifecycle nativo fixture (Child, HTTP, proxy,
probes y cleanup) sin habilitar un endpoint libre ni reinterpretar simulation.

- Nuevo wrapper concreto `CampaignDriver`: receta, provenance nativa, lease
  cooperativo y preflight antes de cada brazo. Un único lease sobrevive a todos
  los runs; los sockets se entregan justo antes de spawn y el Child/listener se
  verifica después. No se reintenta un bind ni se adopta un listener ajeno.
- Ruta de modelo exige confirmación explícita del operador, limitada a receta y
  plazo monotónico. No deserializable de mensajes/JSON; readiness sigue sin ser
  autorización. No CLI que arranque modelos al compilar o ejecutar tests normales.
- Rehearsal usa exactamente recipe argv + Python/entrypoint propio, pero el
  entrypoint de test hace exec del runtime fixture Rust. No hay imports MLX ni
  lectura de pesos reales; provenance solo sobre assets sintéticos nuevos.
  Preflight de recursos inyectado y etiquetado, no una medición de RAM libre.
- Backend model se parametriza desde checkpoint local; validar cuerpo efectivo
  y comparabilidad sin sustituirlo por synthetic-checkpoint. Presupuesto/historial
  y catálogo público pegassus no cambian. Rehearsal no acredita compatibilidad MLX.
- Artefactos `campaign-rehearsal` (host RSS real, model_inference false) o
  `model-campaign` (solo ruta autorizada). Preparación fallida se registra antes
  de cleanup; no se inicia el siguiente brazo con preflight/identidad incierta.
- Hashing por brazo antes de ventana medida (hasta 600 s), seguido de observación
  fresca. Igualdad de provenance entre brazos; cambios abortan, no se ocultan.

Alternativa de duplicar launcher/probes genera lifecycle divergente; se rechaza.
Cambiar fixture para aceptar cualquier ejecutable/endpoint también: el wrapper
restringe la ruta de rehearsal al entrypoint exacto del repositorio. La ruta real
no hereda autorización de la ruta fixture. Nuevos deadlines de startup no cambian
el límite de request 240 s del plan. La exclusión sigue cooperativa y requiere
clearance externo vigente; no se escanean/matan procesos ajenos.

Interfaces de benchmark internas; no wire ni proveedor operativo nuevos. S3-05
solo después de tests/rehearsal y autorización actual. Descendientes sin handles
mantienen cleanup no confirmado; no prometer GPU liberada o paridad multiproceso.

Al superar el rehearsal se actualiza `runner_implemented:true` del plan:
significa implementación de la ruta, **no ejecución/validación de MLX**. El estado
de campaña sigue `specified-not-executed`, authorization_required true. Sustituye
el estado pendiente descrito históricamente en ADR 0010–0012. La interfaz sigue
siendo librería Rust; no se añade un daemon, instalación o CLI de carga automática.
La cancelación se transmite al driver: se comprueba tras hashing y antes de
spawn/nuevas requests para no iniciar carga después de cancelar preparación.
