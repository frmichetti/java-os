# Spec Delta

## Purpose

Permite registrar faixas de endereço virtual cuja falta de página por ausência é recuperável — a thread automaticamente tenta de novo em vez do kernel haltar — a base do "sinal de safepoint" usado pela JVM para parar todas as threads num ponto de segurança sem polling explícito.

## ADDED Requirements

### Requirement: Registro de Faixa de Safepoint
O kernel SHALL permitir registrar uma faixa de endereço virtual, dentro de uma reserva de memória virtual viva, como ponto de poll de safepoint.

#### Scenario: Registro bem-sucedido de uma faixa dentro de uma reserva viva
- **WHEN** uma faixa de endereço virtual dentro de uma reserva de memória virtual viva é registrada
- **THEN** o kernel SHALL aceitar o registro, tornando faltas de página por ausência nessa faixa recuperáveis

#### Scenario: Faixa fora de qualquer reserva viva é rejeitada
- **WHEN** uma faixa de endereço virtual que não corresponde a nenhuma reserva de memória virtual viva é registrada
- **THEN** o kernel SHALL rejeitar o registro, sem tornar nenhuma falta de página recuperável

### Requirement: Falta de Página por Ausência numa Faixa Registrada é Recuperável
Uma falta de página causada por ausência (página não mapeada), dentro de uma faixa registrada como ponto de poll de safepoint, SHALL fazer a thread automaticamente tentar a mesma instrução de novo, em vez de o kernel reportar a exceção e haltar.

#### Scenario: Thread sondando uma página descomprometida tenta de novo em vez de derrubar o kernel
- **WHEN** uma thread acessa um endereço dentro de uma faixa registrada cuja página está descomprometida (ausente)
- **THEN** o kernel SHALL retomar a execução dessa thread exatamente na instrução que causou a falta, sem reportar exceção nem haltar

#### Scenario: Thread continua normalmente assim que a página volta a estar presente
- **WHEN** a página correspondente a uma faixa registrada é comprometida novamente enquanto uma ou mais threads tentam repetidamente acessá-la
- **THEN** a próxima tentativa de cada uma dessas threads SHALL ter sucesso normalmente, sem nenhuma falta de página

### Requirement: Falta de Página Fora de uma Faixa Registrada Continua Fatal
Uma falta de página que não seja uma ausência dentro de uma faixa registrada (qualquer outra falta de página, incluindo violações de permissão, e ausências fora de qualquer faixa registrada) SHALL continuar sendo tratada como evento fatal, exatamente como antes desta capability existir.

#### Scenario: Violação de permissão numa faixa registrada continua fatal
- **WHEN** uma thread escreve numa página somente-leitura dentro de uma faixa registrada como ponto de poll de safepoint
- **THEN** o kernel SHALL reportar a exceção e haltar, em vez de tratar a falta como recuperável

#### Scenario: Ausência fora de qualquer faixa registrada continua fatal
- **WHEN** uma thread acessa um endereço não mapeado que não pertence a nenhuma faixa registrada como ponto de poll de safepoint
- **THEN** o kernel SHALL reportar a exceção e haltar, exatamente como antes desta capability existir
