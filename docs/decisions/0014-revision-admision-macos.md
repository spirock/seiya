# ADR 0014 · revisión de admisión macOS sin cargar modelos

2026-10-20 · revisión autorizada de S3-05. Estado: diagnóstico implementable;
**no se aprueba cambiar el gate ni ejecutar una campaña por este ADR**.

## Problema

`minimum_free_bytes=2 GiB` exige páginas ya libres. macOS puede mantener cachés
recuperables con pocas páginas libres y presión normal. El criterio actual es
una condición conservadora elegida por operador, no una estimación de capacidad
ni una reserva para Gemma. Incluso superarlo no demuestra que el modelo quepa:
no incluye un presupuesto de carga, KV, temporales y margen del host.

## Evidencia de interfaces consultadas

- SDK local MacOSX.sdk, `usr/include/os/proc.h`, líneas46–87:
  `os_proc_available_memory` mide margen hasta límite dirty del **proceso actual**,
  puede invalidarse instantáneamente y está marcado API_UNAVAILABLE(macos).
  No usarlo como RAM disponible de macOS ni margen de un futuro runtime hijo.
- SDK local, `Metal.framework/Headers/MTLDevice.h`, líneas517–523:
  `recommendedMaxWorkingSetSize` aproxima recursos utilizables con buen rendimiento,
  no bytes libres ni reserva exclusiva. `currentAllocatedSize` no contabiliza
  todos los consumidores externos y no debe restarse como si lo hiciera.
- `man memory_pressure` del host describe una herramienta capaz de provocar/
  simular presión. Se consultó el manual, no se ejecutó memory_pressure ni sus
  modos de presión. No depender de flags no documentados como estimación segura.
- Código XNU consultado como documentación, no copiado:
  https://github.com/apple-oss-distributions/xnu/blob/main/bsd/kern/kern_memorystatus.c
  distingue presión, límites por proceso y mecanismos específicos de plataforma.
  No se asume que main corresponda exactamente al kernel instalado ni se enlazan
  APIs privadas de XNU por encontrar un símbolo en sus fuentes.

## Decisión

1. Mantener `assess` y la receta actual sin relajar el umbral. Etiquetar su método
   como free-pages-only, nunca como available/safe-to-load.
2. Añadir `review_pressure_window`: evaluación **diagnóstica**, no admisión, de
   3–16 observaciones en orden estricto, separadas como máximo2 s, ventana de
   al menos2 s y último dato≤5 s. Rechazar relojes futuros/regresivos, memoria
   inconsistente, faltantes, hardware cambiante y clearance ausente. Cualquier
   presión warning/critical/unknown impide declarar la ventana normal.
3. Una ventana normal solo acredita lo observado durante esos instantes. No
   suma inactive/free/speculative/purgeable, no convierte porcentaje a bytes,
   no reserva RAM y nunca devuelve autorización de carga. El informe declara
   headroom seguro desconocido y presupuesto runtime no validado.
4. Probar política con fixtures sin probes. Auditoría nativa opt-in solo sysctl/
   vm_stat y espera breve, sin pesos/MLX/procesos ajenos. Conservar la serie cruda
   y el rechazo del gate actual junto al diagnóstico; no cambiar la etiqueta de
   una observación normal a campaña realizada o baseline medida.

## Alternativas y siguiente paso

- Rechazadas: bajar2 GiB para que pase; sumar inactive como memoria disponible;
  admitir con presión normal o tamaño de shards como único presupuesto; provocar
  presión para calibrar capacidad; cerrar aplicaciones ajenas.
- Un criterio nuevo requerirá presupuesto explícito del runtime/contexto/cache/
  temporales y margen del SO con procedencia y límites. Alternativa experimental:
  carga controlada autorizada con monitorización continua y parada propia ante
  presión, claramente distinta de una garantía previa de capacidad. Antes de
  implementarla hay que resolver lifecycle/cancelación durante carga/generación
  y acordar ese riesgo, no fabricar ese permiso con una ventana normal.
- Migración: campos de diagnóstico aditivos, sin cambiar API Seiya, modelos,
  receta, umbral o el estado de S3-05 (bloqueada). No reabre KV/MoE aplazados.
