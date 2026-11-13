# ADR0058 · host portable sin bifurcar el dominio de exclusión

Estado: implementación M2, no activación de modelos reales.

## Alternativas

Aceptar cualquier coordination_dir como autoridad permite saltar una cuarentena
con otro JSON: rechazado. Mantener solo BASE del ensayo impide portar a otro Mac:
rechazado como destino final. Se elige autoridad estable por usuario y puente
cooperativo al dominio legado cuando exista; sin borrar ni trasladar archivos.

## Decisión

El arranque portable exige coordination_dir igual a la ruta canónica privada
HOME/Library/Application Support/Seiya/coordination. No se crea ese directorio ni
se migra estado por arrancar. El chequeo offline v1 sigue validando estructura;
la autoridad se valida adicionalmente al arrancar. HOME forma parte del entorno
de confianza del operador, no un dato enviado por el navegador/modelo.

Lease: flock no bloqueante en lock estable nunca eliminado. Cuando el dominio
legado existe, adquirir también su lock existente (no crear uno que simule
coordinación). Quarantine de imagen/par se comprueba dentro de ambos locks.
Cleanup incierto escribe marcadores en ambos dominios; si falla persistirlos,
retener los locks durante la vida del proceso. Recuperación solo del legado no
limpia la autoridad nueva. Ninguna recuperación automática se incorpora en M2.

Compatibilidad supone launchers cooperativos, mismo usuario/HOME y dominio
legado no movido/eliminado/reaprovisionado durante una sesión. No protege de un
operador que cambie HOME o quite locks, ni de binarios externos no cooperativos.
No coordinar otra instalación legacy en una ruta distinta sin revisión explícita.
Se mantienen inventario vacío de runtime compartido y límites/cleanup existentes.

## Integración y rollback

Entorno inyectado en chat, imágenes, directorios por turno y cuarentena. APIs
antiguas conservan defaults del ensayo y el mismo lock legado. Binary/assets
imagen siguen fijados por hashes/manifiesto/procedencia; configurar paths no
habilita cualquier motor. Solo runtime Ollama compatible con el ensayo para LLM.
Sin selección implícita de proveedor, descargas o fallback.

CLI portable requiere aprobación explícita, JSON, LAN privada8015, TLS y modo
opcional chat-only. Arranque sin warmup/chat automático; settings CAS existentes
prevalecen sobre features JSON, que solo inicializa un perfil nuevo. Modo chat-only
no adjunta Qwen/imagen aunque haya preferencias guardadas activadas.
No cambiar CLI antigua o instalación activa. Rollback a launcher anterior sin
borrar datos: si hay cuarentena replicada, el antiguo también debe rechazarla.
M3 sigue pendiente: no se ofrece aún launcher gráfico ni loopback sin TLS.
