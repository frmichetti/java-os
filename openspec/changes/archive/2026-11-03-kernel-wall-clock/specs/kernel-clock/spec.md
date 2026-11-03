# Spec Delta

## Purpose

Fornece relógio de parede (data/hora real desde a época Unix) e relógio monotônico de alta resolução (nanossegundos decorridos, nunca retrocede) — a base de `clock_gettime` POSIX, usada por `System.currentTimeMillis()`/`System.nanoTime()` do HotSpot.

## ADDED Requirements

### Requirement: Relógio de Parede com Época Unix
O kernel SHALL expor a data/hora atual, em nanossegundos desde a época Unix (1970-01-01 00:00:00 UTC), derivada do relógio de hardware do sistema.

#### Scenario: Data/hora relatada corresponde à hora real do sistema
- **WHEN** o relógio de parede é consultado
- **THEN** o valor retornado, convertido de volta para data/hora civil, SHALL corresponder à data/hora configurada no relógio de hardware do sistema no momento do boot, mais o tempo decorrido desde então

#### Scenario: Relógio de parede nunca retrocede durante a execução do kernel
- **WHEN** o relógio de parede é consultado duas vezes em sequência, sem nenhum ajuste de hora entre as consultas
- **THEN** a segunda leitura SHALL ser maior ou igual à primeira

### Requirement: Relógio Monotônico de Alta Resolução
O kernel SHALL expor um contador de tempo decorrido, em nanossegundos, com resolução mais fina que o período do timer periódico já existente (`kernel-timer`), que nunca retrocede e é independente de qualquer ajuste do relógio de parede.

#### Scenario: Relógio monotônico distingue intervalos menores que um tick do timer periódico
- **WHEN** o relógio monotônico é consultado duas vezes, com uma operação rápida (mais curta que um período do timer periódico) entre as duas consultas
- **THEN** a diferença entre as duas leituras SHALL refletir esse intervalo com resolução de nanossegundos, não arredondada para o período do timer periódico

#### Scenario: Relógio monotônico sempre cresce, mesmo que o relógio de parede seja hipoteticamente ajustado
- **WHEN** o relógio monotônico é consultado antes e depois de qualquer evento que pudesse afetar o relógio de parede
- **THEN** a leitura do relógio monotônico SHALL continuar estritamente não-decrescente, independente de qualquer mudança no relógio de parede
