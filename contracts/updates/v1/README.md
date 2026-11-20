# Actualizaciones estables v1 · contrato offline experimental

`stable.example.json` es una fixture sintética, no una release ni una
autorización de red. `seiya-contracts::update_manifest` valida JSON cerrado
de hasta 4096 bytes, versión estable `MAJOR.MINOR.PATCH`, SHA hexadecimal
minúsculo de 40 caracteres y URL exacta de la release correspondiente en
`spirock/seiya`. Solo una versión estrictamente posterior es candidata.

No verifica existencia, procedencia del SHA, firma o integridad del ejecutable;
no consulta GitHub, abre enlaces, instala ni funciona como actualizador.
Versiones prerelease se rechazan hasta definir su canal independiente.
