# ADR 0008 · parsers macOS, sin ejecución

2026-10-19 · S3-02. Extiende seiya-observe con funciones puras sobre texto limitado.
Sin comando, red o proceso real en esta tarea; formatos para ejecución futura
con locale C. No se copia código Colibri ni se añade dependencia.

- RSS: salida sin headers de `ps -p PID -o pid=,rss=`; exactamente una fila y dos
  enteros ASCII no negativos, PID positivo esperado. Salida vacía no prueba exit:
  se rechaza; distinguir exit/permisos/identidad pertenece al adaptador S3-03.
- RSS no contiene identidad de inicio. No derivarla de lstart con precisión de
  segundos ni afirmar ownership por coincidencia de PID. El adaptador debe verificar
  la identidad fuerte del proceso creado antes/después de medir; si no, unsupported.
- vm_stat: header y page size explícitos, potencia de dos entre 1024 y 65536.
  Guardar solo contadores de páginas conocidos; aceptar/validar otros contadores
  sin conservar sus nombres. Ausencias siguen ausentes, nunca cero. Duplicados
  y valores ambiguos/desbordados se rechazan. No derivar "RAM disponible".
- swap: formato `vm.swapusage`, total/used/free en orden, unidad B/K/M/G/T binaria,
  hasta dos decimales y sufijo `(encrypted)` opcional. Conversión entera con u128,
  floor a bytes (resolución del texto, no precisión física). Comprobar consistencia
  dentro de la tolerancia de redondeo; no confundir swap con memoria propia.
- Máximo 16 KiB/128 líneas, RSS/swap una fila, números solo ASCII. Errores tipados
  sin incluir texto del proveedor. Rechazar locales/unidades no soportados en vez
  de adivinar. Estas cotas son también responsabilidad de lectura del futuro
  ejecutor: parsear texto acotado no limita por sí solo un stdout de proceso vivo.

Fixtures sintéticas prueban conversiones, formatos, ausencias y fallos, no que
ps tenga todos los datos de la GPU ni un proceso sea realmente propio. S3-03
elegirá el mecanismo de identidad y validará el adaptador antes del runner.
