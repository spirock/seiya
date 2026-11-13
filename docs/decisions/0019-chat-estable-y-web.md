# ADR 0019 · estabilizar chat antes de ampliar lectura web

2026-10-24 · decisión de alcance/diseño; implementación pendiente.
Contrato: [chat estable y web](../CONTRATO_CHAT_ESTABLE_Y_WEB.md).

## Contexto

Gemma respondió en un smoke manual, pero el monitor lo detuvo después por presión
del host. No hay trazas suficientes para atribuir la causa ni baseline S3-05.
La búsqueda web devuelve snippets/portadas: el ejemplo del usuario ofrece enlaces,
no un resumen. Leer artículos introduce red no confiable y contexto adicional.

## Decisión

1. Primero instrumentación/perfil acotado y parada visible; después lectura segura,
   y finalmente prueba combinada. Se puede desarrollar con fixtures mientras las
   campañas reales estén bloqueadas, sin anunciar lector ni perfil estable.
2. Nueva lógica compartida de políticas, presupuestos, fetch/extracción y tipos en
   Rust. Python conserva coordinación/UI actuales hasta contar con compatibilidad
   y rollback. No duplicar herramientas OpenCode ni promover el servidor Rust.
3. MVP de lectura: IDs de resultados del run, HTTPS público, secuencial, sin JS,
   sesiones del navegador, autenticación, crawler o URLs arbitrarias del modelo.
4. Bridge propuesto para primer prototipo: helper Rust local opt-in, ruta absoluta
   fijada por operador, invocado sin shell con JSON por stdin/stdout acotados y
   deadline. La implementación deberá decidir/registrar biblioteca+CLI, esquema
   interno versionado, framing, identidad del binario, cancelación e hijos propios
   en una ADR de bridge antes de código. Este ADR no fija todavía un ABI público.
5. Tres resultados separados: búsqueda, lectura y síntesis. IDs/citas validados
   por host; soporte semántico evaluado con fixtures y revisión, no por HTTP 200.

## Alternativas y consecuencias

- Solo mejorar prompt: puede mejorar el resumen de snippets, no añade lectura ni
  soluciona presión. No aceptado como entrega completa.
- Fetch Python rápido o curl con URL libre: duplica política compartida y amplía
  SSRF/rebinding; rechazado. El uso actual de curl a buscador fijo no demuestra
  seguridad para destinos arbitrarios.
- Browser/Safari automatizado: implica permisos, sesiones privadas, JS y mayor
  coste; excluido del MVP. No habilitar Apple Events/Accessibility como atajo.
- Helper opt-in: mantiene un proceso/framing que hay que acotar y empaquetar;
  reduce acoplamiento con Python, pero no será seguro solo por estar en Rust.
- FFI o servicio HTTP nuevo: posibles alternativas del bridge, con más complejidad
  de distribución/lifetime o superficie de autenticación; requieren evidencia
  antes de sustituir el helper propuesto. No abrir nuevos puertos por este plan.
- Bajar gates/reiniciar siempre: oculta la inestabilidad; rechazado. Sin runtime
  descargable/cancelable probado, mantener cuarentena y cleanup confirmado.

## Migración y autorización

Contratos nativo/OpenAI intactos. Esquemas de bridge/eventos nuevos se versionan
antes de implementar. Perfil y feature flag nuevos, sin reescribir configuración
activa, pesos, histórico o instaladores. Sin lector disponible conservar modo
snippets con limitación visible; nunca inventar una lectura exitosa.

S3-05/S3-06 siguen abiertas/bloqueadas. Sprint 04 es planificado, no reemplaza su
evidencia ni las marca hechas. KV/MoE siguen aplazados. Aprobar documentos no
autoriza campañas, red real, instalación, cierre de apps o cambios del servidor.
