# ADR0054 · prueba Gemma E2B de un turno

Prueba autorizada de funcionamiento Gemma breve/aislada, guardas
activas y sin retry automático. Reutilizar Chat/Session/OwnedChild existentes;
variante CLI aditiva `seiya-image-web --gemma-smoke ROOT_NUEVO --approval
one-isolated-gemma-e2b-response-with-memory-guards`, sin bind web/TLS o preparación
de imágenes/Qwen. Alternativa descartada: reiniciar la web completa para pedir
un marcador; cargaría componentes ajenos al ensayo. No nuevo wire/modelo.

Preflight normal/ventana2s/swap estable32MiB/disco4GiB, inventario11434 vacío y
lease común antes de preparar blobs existentes verificados. El turno vuelve a
comprobar exclusión y presión antes de spawn. Contexto4096,≤256tokens y120s,
warning acumulado10s/swap+2GiB, critical/unknown/stale inmediatos. No modificar
estos límites a raíz de un fallo; cancelar/cerrar solo árbol propio y conservar
cuarentena si cleanup no confirma. Un intento, sin fallback remoto.

RSS muestreado por PID verificado de OwnedChild junto a muestras del host; no
sumarlo ni equipararlo a footprint, Metal o MLX activo. El pico puede no estar
muestreado. Registrar si llegó a intentarse inferencia, marcador/uso, cleanup,
puertos/lease y estado final, sin presentar preflight como respuesta real.
Rollback: omitir variante CLI, conservar evidencias/modelos/configuración.
