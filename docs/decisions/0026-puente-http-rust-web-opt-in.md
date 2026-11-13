# ADR 0026 · puente web/inferencia Rust opt-in

2026-10-26 · dependencia S4-04 para cerrar S4-02c, no activación ni lector web.

## Decisión

- Reutilizar endpoints existentes de seiya-server (/health y /v1/chat/completions)
  para inferencia/estado. No crear daemon o copiar política de cuarentena en Python.
  Perfil Python runtime_bridge opt-in, fast/un único worker MLX. Default intacto.
- Nuevo adaptador Python mínimo de transporte: solo HTTP numérico127.0.0.1,
  puerto explícito, /v1, sin proxies/redirects, límites de bytes/plazos. Credencial
  opcional por nombre de variable de entorno; nunca en config/binary/logs.
- Handshake verifica header experimental de chat y runtime_status versión1/2,
  más runtime_bridge versión1/model_binding hex-utf8-v1 (solo opt-in). Un Rust
  antiguo sin ese anuncio no puede aceptar silenciosamente un header desconocido.
  no confunde snapshots del supervisor con health. No implica pesos cargados.
- Modelo público pegassus al puente; header x-seiya-model-binding contiene hex
  UTF-8 del identificador backend configurado. Rust comprueba coincidencia antes
  de body/slot/efectos, tras Host/Origin/auth. Evita sustitución accidental de modelo
  o bucles; no es autenticación, hash de pesos ni permiso de cargar. No loguear.
- Transporte reenvía messages/tools/presupuesto/temperatura sin ejecutar tools.
  No consenso en este perfil. Rust conserva su worker/cola/lease, errores opt-in y
  no retries. Seiya no duplica herramientas OpenCode.
- Fallo ambiguo del enlace Python→Rust bloquea nuevas generaciones en ese cliente
  hasta recuperación explícita/nueva instancia; health de solo lectura no rearma.
  No interpretar caída del puente como modelo apagado o presión. Sin fallback.
- UI consulta health autenticado, usa textContent y estados cerrados. Distingue
  coordinador/runtime/modelo desconocido y memoria no integrada. No inventa loaded,
  progreso o retries. Error conserva borrador y chat; ninguna migración localStorage.

## Alternativas y evolución

Helper stdin por inferencia añade arranque y perdería la cuarentena persistente:
rechazado para este camino. Python directo sigue default/rollback. Web fetch no
se implementa por usar este puente: decisión previa ADR0019 de helper Rust para
lectura queda separada. Antes de S4-05 debe congelarse su esquema/IPC/cotas propios;
runtime_bridge no autoriza red a páginas ni anuncia tools planned.

Config nueva como copia explícita, sin instalación o reinicio automático, nunca
coordinadores competidores en runtime. Puede haber dos saltos loopback: medir
latencia, no prometer aceleración. Pruebas de fixtures sin modelos/red externa.
S4-04 queda parcial: transporte/estado resuelto; esquema del lector aún pendiente.
