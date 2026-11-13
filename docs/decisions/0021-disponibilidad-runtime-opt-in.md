# ADR 0021 · disponibilidad observada del runtime, opt-in

2026-10-25 · S4-02a, primer incremento de S4-02. No completa supervisión de RAM,
cleanup de procesos, puente Python o UI. Contrato base:
[chat estable y web](../CONTRATO_CHAT_ESTABLE_Y_WEB.md).

## Decisión antes de interfaces

- Config Rust nueva `runtime_status:true`, booleano y default false; requiere
  runtime configurado. Solo habilita errores y observaciones de disponibilidad.
  No inicia, sondea, carga, reintenta ni recupera un runtime automáticamente.
- Campo adicional `/health.runtime_status` con versión1, observación cached del
  último intento, estado de admisión, modelo residente siempre unknown y control
  de memoria explícitamente not_integrated. Health del coordinador sigue200.
  Ninguna inferencia se envía al consultar health. No promete readiness duradera.
- Nueva categoría interna `NotConnected`: solo errores is_connect del send HTTP,
  no timeouts ni errores al recibir body. Con opt-in, primer fallo devuelve503,
  `error.code:runtime_unavailable`, mensaje fijo sin rutas/URLs. Sin Retry-After,
  porque no se conoce cuándo estará disponible. No atribuirlo a presión.
- Si ya se pudo enviar trabajo y la finalización no se conoce, mantener
  cuarentena. No convertir un timeout/disconnect/body inválido en runtime apagado.
  Error posterior503 con code runtime_state_unconfirmed. Un HTTP/model JSON no
  válido continúa con el tratamiento conservador anterior.
- Fallo NotConnected opt-in detiene nuevas admisiones e impide a peticiones ya
  en cola volver a llamar al backend. No hay reset HTTP ni reconnect transparente;
  recuperación explícita del operador en una nueva instancia, después de verificar
  que el runtime previo no sigue generando. No reiniciar servidor OpenCode.
- Completion JSON válida a nivel de transporte acredita final de esa petición,
  incluso si falla su semántica. No acredita memoria, siguiente petición, carga
  persistente o seguridad de más contexto. Modelo residente permanece unknown.
- Default false conserva error JSON/status/messages y health anteriores; el
  transporte puede refinar su categoría interna sin anunciar capacidades nuevas.
  Las fixtures nuevas son aditivas, no modifican wire nativo SessionRequest.

## Alternativas y consecuencias

Un GET /models por petición añade coste, puede fallar sin demostrar estado del
modelo y no garantiza siguiente POST: pospuesto. Reiniciar automáticamente o
seguir retries tras timeout puede solapar generaciones: rechazado. Leer presión
desde health confunde observación de host con capacidad de carga: rechazado.
Modificar el prototipo Python duplicando política Rust también se pospone: falta
bridge versionado S4-04. No fingir una UI operativa que Rust aún no sirve.

## Pruebas y migración

Mocks deterministas para ausencia, body/timeout inciertos, cola, desconexión del
consumidor y shutdown; HTTP local efímero solo con servidores sintéticos propios.
Sin red externa, modelos, procesos del usuario ni cambios de OpenCode. Primero
schema/fixtures y después código. Config activa sin cambios, flag solo en perfiles
de pruebas nuevos. Rollback: retirar flag del perfil experimental; no migrar chats.
