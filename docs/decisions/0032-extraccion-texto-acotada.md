# ADR0032 · extracción offline acotada

S4-06: parser HTML5 y dependencias Cargo fijadas, sin red
de lectura. Se elige tokenizer html5ever0.35 (MIT/Apache2, compatible con MSRV),
sin construir DOM ni navegador, estilos computados o JS. No se copia un motor.

Entrada ya validada por S4-05: MIME HTML/texto, UTF-8 estricto y≤1MiB. Tokenización
HTML5 para entidades y script/rawtext, con selección conservadora propia. No se
afirma equivalencia a un DOM del navegador ni legibilidad general. Preferir texto
de main/article si existen; en otro caso cuerpo filtrado. Excluir scripts/estilos,
head/nav/form/controles/template/iframe/objetos/medios y hidden/aria-hidden. No
ejecutar CSS, JS, enlaces, redirects del cuerpo, feeds o sesiones Safari. El estilo
computado y anti-bot/paywall no se eluden; fixtures no validan sitios reales.

Máximo256 elementos anidados/atributos por tag y100000 tokens; desborde falla
cerrado, no documento parcial fingido. Hasta20000 puntos Unicode por documento,
30000 por batch; solo recorte de texto válido produce text_truncated=true. Mínimo
80 caracteres útiles como heurística inicial, no rúbrica de calidad ni prueba de
artículo/fecha. No extraer fechas como metadatos: texto de time se conserva como
dato no confiable. Ni copyright ni query/hoy equivalen a publicación verificada.

Selección de fuentes para inferencia: solo IDs de este run presentes en el frame
validado y texto realmente entregado;8000 caracteres/fuente y16000 acumulados,
además del presupuesto de tokens que S4-07 debe aplicar. No aceptar URLs del modelo.
Sin presupuesto de tokens comprobado, esta selección no demuestra ajuste al contexto.
No truncar esquemas/mensajes para ocultarlo. Texto se entrega como datos: ninguna
instrucción incrustada cambia permisos o se eleva a sistema. La UI de S4-07 debe
renderizar mediante textContent, nunca HTML de fuentes sin escapar.

Rollback: lector no habilitado en CLI/servicio, volver a snippets con limitación
visible. No cambio de identidad Pegassus, protocolo nativo, /v1 ni historial.
