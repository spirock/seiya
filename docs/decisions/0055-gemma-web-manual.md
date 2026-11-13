# ADR0055 · prueba interactiva, no marcador del agente

Usuario aclara «yo quiero hacer la prueba entrando a la web». Nuevo perfil
aditivo --gemma-web del host Rust existente, aprobación explícita
temporary-manual-gemma-only-LAN-web. LAN/TLS8015 y diseño actual; Gemma real bajo
demanda, Qwen/imágenes apagados. No mensaje de warmup/smoke del agente ni prueba
automática antes de entregar. Alternativa descartada: usar perfil conjunto que
prepara hashes de imagen e inicia marcador antes de abrir acceso.

Reusar transporte/guardas/RSS/cleanup por turno y configuración persistente.
Preflight normal/swap estable/disco/inventario11434 vacío/lease antes de preparar
solo blobs Gemma existentes. No prometer respuesta por superar preflight; cargar
solo al POST manual del navegador. Critical/unknown/stale detienen sin retry.
Un runtime privado propio por turno,≤120s/≤256tokens/contexto4096, sesión≤30min.

Bootstrap probe separado para verificar TLS/health sin POST chat y sin consumir
enlace del usuario. Evidencia de arranque distingue ready/automatic_chat_probefalse
y chat_smoke_validatedfalse. Conversación y acceso desde otro PC solo los acredita
el usuario. Rollback: expirar/parar host propio; no modelos/evidencias/configs
borrados, servidores compartidos modificados o umbrales relajados.
