# kernel-syscalls Specification

## Purpose

Fornece a interface de chamada de sistema (`syscall`/`sysret`) pela qual código em modo usuário solicita serviços do kernel, com uma stack de kernel confiável, uma convenção de ABI explícita, e despacho por número de chamada.

## Requirements

### Requirement: Chamada de Sistema Troca para Stack de Kernel Confiável
Ao receber uma chamada de sistema, o kernel SHALL passar a executar sobre a stack de kernel da thread chamadora antes de interpretar qualquer dado vindo do modo usuário, em vez de continuar sobre a stack de usuário (não confiável) da thread chamadora.

#### Scenario: Chamada de sistema nunca processa dados sobre a stack de usuário
- **WHEN** uma thread em modo usuário executa a instrução de chamada de sistema
- **THEN** o handler da chamada SHALL executar inteiramente sobre a stack de kernel dessa thread, nunca sobre a stack de usuário dela

### Requirement: Retorno ao Modo Usuário Após a Chamada
Após atender uma chamada de sistema, o kernel SHALL retornar a execução ao modo usuário, no ponto exatamente seguinte à instrução de chamada, com o resultado da chamada disponível para o código chamador.

#### Scenario: Thread chamadora retoma a execução em modo usuário com o resultado
- **WHEN** o kernel termina de atender uma chamada de sistema
- **THEN** a thread chamadora SHALL retomar a execução em modo usuário imediatamente após a instrução de chamada, com o valor de retorno da chamada acessível a ela

### Requirement: Despacho por Número de Chamada
O kernel SHALL identificar qual serviço está sendo solicitado por um número de chamada explícito, e SHALL rejeitar de forma explícita um número de chamada não reconhecido, em vez de executar um serviço arbitrário ou travar.

#### Scenario: Número de chamada inválido é rejeitado explicitamente
- **WHEN** uma chamada de sistema é feita com um número de chamada que o kernel não reconhece
- **THEN** o kernel SHALL retornar um erro explícito para quem chamou, em vez de executar um serviço não intencional, travar, ou produzir comportamento indefinido

### Requirement: Chamada de Escrita no Console
O kernel SHALL fornecer uma chamada de sistema que escreve uma sequência de bytes fornecida pelo chamador no console de saída do kernel.

#### Scenario: Dados escritos pela chamada aparecem no console
- **WHEN** código em modo usuário invoca a chamada de escrita com uma sequência de bytes
- **THEN** essa sequência de bytes SHALL aparecer no console de saída do kernel

### Requirement: Chamada de Consulta do Contador de Ticks
O kernel SHALL fornecer uma chamada de sistema que devolve ao chamador o valor atual do contador de ticks do kernel.

#### Scenario: Valor devolvido reflete o contador real do kernel
- **WHEN** código em modo usuário invoca a chamada de consulta de ticks
- **THEN** o valor devolvido SHALL corresponder ao contador de ticks do kernel no momento da chamada

### Requirement: Chamada de Reserva de Memória Virtual Anônima
O kernel SHALL fornecer uma chamada de sistema que reserva uma faixa de memória virtual anônima de tamanho pedido pelo chamador, sem comprometer memória física, e devolve o endereço virtual base da faixa reservada.

#### Scenario: Reserva bem-sucedida devolve um endereço base
- **WHEN** um processo em modo usuário invoca a chamada com um tamanho válido e há espaço virtual livre suficiente
- **THEN** o kernel devolve o endereço virtual base de uma faixa recém-reservada daquele tamanho (arredondado para página), sem comprometer nenhuma memória física

#### Scenario: Tamanho inválido ou sem espaço livre é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com tamanho zero, ou não há espaço virtual livre suficiente
- **THEN** o kernel devolve o sentinela de erro, sem reservar nenhuma faixa

### Requirement: Chamada de Comprometimento de Sub-região Reservada
O kernel SHALL fornecer uma chamada de sistema que compromete, com as permissões de acesso pedidas pelo chamador, uma sub-região de uma faixa previamente reservada, identificada pelo endereço virtual e tamanho da sub-região.

#### Scenario: Comprometimento bem-sucedido torna a sub-região acessível
- **WHEN** um processo em modo usuário invoca a chamada com um endereço e tamanho dentro de uma faixa reservada ainda não comprometida naquela sub-região
- **THEN** o kernel compromete a sub-região e o chamador consegue ler e escrever nela conforme as permissões pedidas

#### Scenario: Página comprometida nunca concede acesso privilegiado de kernel
- **WHEN** um processo em modo usuário invoca a chamada, com qualquer combinação de permissões pedidas
- **THEN** a página resultante SHALL permanecer acessível a partir do modo usuário, independentemente do valor bruto de permissões recebido do chamador

#### Scenario: Endereço fora de uma reserva, ou sub-região já comprometida, é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um endereço/tamanho que não corresponde a uma sub-região livre dentro de uma faixa reservada viva
- **THEN** o kernel devolve o sentinela de erro, sem comprometer nenhuma memória física

### Requirement: Chamada de Descomprometimento de Sub-região Comprometida
O kernel SHALL fornecer uma chamada de sistema que descompromete uma sub-região previamente comprometida, identificada por endereço virtual e tamanho, liberando sua memória física e preservando a reserva virtual.

#### Scenario: Descomprometimento bem-sucedido libera a memória física
- **WHEN** um processo em modo usuário invoca a chamada com um endereço/tamanho totalmente comprometido
- **THEN** o kernel descompromete a sub-região e um acesso subsequente a ela falha, enquanto a faixa reservada permanece disponível para um novo comprometimento

#### Scenario: Sub-região não totalmente comprometida é rejeitada
- **WHEN** um processo em modo usuário invoca a chamada com um endereço/tamanho que não está totalmente comprometido
- **THEN** o kernel devolve o sentinela de erro, sem alterar nenhum estado de comprometimento

### Requirement: Chamada de Alteração de Proteção de Sub-região Comprometida
O kernel SHALL fornecer uma chamada de sistema que altera as permissões de acesso de uma sub-região já comprometida, identificada por endereço virtual e tamanho, sem alterar seu conteúdo.

#### Scenario: Alteração de proteção bem-sucedida aplica as novas permissões
- **WHEN** um processo em modo usuário invoca a chamada com um endereço/tamanho totalmente comprometido e novas permissões
- **THEN** o kernel aplica as novas permissões de acesso à sub-região, preservando seu conteúdo e os frames físicos que a sustentam

#### Scenario: Sub-região não totalmente comprometida é rejeitada
- **WHEN** um processo em modo usuário invoca a chamada com um endereço/tamanho que não está totalmente comprometido
- **THEN** o kernel devolve o sentinela de erro, sem alterar nenhuma permissão

### Requirement: Chamada de Liberação de Reserva
O kernel SHALL fornecer uma chamada de sistema que libera por completo uma faixa de memória virtual previamente reservada, identificada pelo seu endereço virtual base, descomprometendo qualquer sub-região ainda comprometida.

#### Scenario: Liberação bem-sucedida devolve a faixa ao espaço livre
- **WHEN** um processo em modo usuário invoca a chamada com o endereço base de uma faixa reservada viva
- **THEN** o kernel descompromete qualquer sub-região ainda comprometida, libera a faixa inteira e esse espaço de endereço volta a ficar disponível para reservas futuras

#### Scenario: Endereço que não corresponde a uma reserva viva é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um endereço que não é a base de nenhuma reserva viva
- **THEN** o kernel devolve o sentinela de erro, sem liberar nenhuma faixa

### Requirement: Chamada de Criação de Mutex Nomeado
O kernel SHALL fornecer uma chamada de sistema que cria um novo mutex nomeado e devolve um identificador (`handle`) opaco para ele, utilizável pelas demais chamadas de mutex.

#### Scenario: Criação bem-sucedida devolve um handle utilizável
- **WHEN** um processo em modo usuário invoca a chamada de criação de mutex
- **THEN** o kernel devolve um `handle` que identifica um mutex novo, inicialmente livre

### Requirement: Chamada de Aquisição Bloqueante de Mutex Nomeado
O kernel SHALL fornecer uma chamada de sistema que adquire o mutex identificado pelo `handle` informado, bloqueando a thread chamadora (sem busy-wait) se o mutex já estiver tomado por outra thread.

#### Scenario: Aquisição de mutex livre retorna imediatamente
- **WHEN** um processo em modo usuário invoca a chamada com o `handle` de um mutex livre
- **THEN** o kernel marca o mutex como tomado pela thread chamadora e a chamada retorna com sucesso

#### Scenario: Aquisição de mutex já tomado bloqueia até ele ser liberado
- **WHEN** um processo em modo usuário invoca a chamada com o `handle` de um mutex já tomado por outra thread
- **THEN** a thread chamadora é bloqueada (sem busy-wait) até o mutex ser liberado e então adquirido por ela

#### Scenario: Handle desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um `handle` que não corresponde a nenhum mutex criado
- **THEN** o kernel devolve o sentinela de erro, sem bloquear a thread chamadora

### Requirement: Chamada de Liberação de Mutex Nomeado
O kernel SHALL fornecer uma chamada de sistema que libera o mutex identificado pelo `handle` informado, acordando uma thread em espera para adquiri-lo, se houver.

#### Scenario: Liberação bem-sucedida acorda uma thread em espera
- **WHEN** um processo em modo usuário invoca a chamada com o `handle` de um mutex tomado, havendo uma ou mais threads bloqueadas tentando adquiri-lo
- **THEN** o kernel libera o mutex e acorda exatamente uma dessas threads, que passa a deter o mutex

#### Scenario: Handle desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um `handle` que não corresponde a nenhum mutex criado
- **THEN** o kernel devolve o sentinela de erro

### Requirement: Chamada de Criação de Variável de Condição Nomeada
O kernel SHALL fornecer uma chamada de sistema que cria uma nova variável de condição nomeada e devolve um identificador (`handle`) opaco para ela, utilizável pelas demais chamadas de variável de condição.

#### Scenario: Criação bem-sucedida devolve um handle utilizável
- **WHEN** um processo em modo usuário invoca a chamada de criação de variável de condição
- **THEN** o kernel devolve um `handle` que identifica uma variável de condição nova, sem nenhuma thread esperando nela

### Requirement: Chamada de Espera em Variável de Condição Nomeada
O kernel SHALL fornecer uma chamada de sistema que libera o mutex nomeado informado e bloqueia a thread chamadora na variável de condição nomeada informada, como uma única operação atômica, reaquirindo o mesmo mutex antes de retornar quando a thread for notificada.

#### Scenario: Espera bem-sucedida libera o mutex e bloqueia a thread
- **WHEN** um processo em modo usuário invoca a chamada com os `handles` de uma variável de condição e de um mutex que a thread chamadora detém
- **THEN** o kernel libera o mutex, bloqueia a thread chamadora na variável de condição e, ao ser notificada, reaquire o mesmo mutex antes da chamada retornar

#### Scenario: Handle de variável de condição ou de mutex desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada com um `handle` de variável de condição ou de mutex que não corresponde a nenhum objeto criado
- **THEN** o kernel devolve o sentinela de erro, sem bloquear a thread chamadora nem alterar o estado do mutex

### Requirement: Chamada de Notificação de Variável de Condição Nomeada
O kernel SHALL fornecer chamadas de sistema que notificam, respectivamente, uma ou todas as threads atualmente esperando na variável de condição identificada pelo `handle` informado.

#### Scenario: Notificar uma thread acorda a que espera há mais tempo
- **WHEN** um processo em modo usuário invoca a chamada de notificar uma thread, com o `handle` de uma variável de condição com uma ou mais threads esperando
- **THEN** o kernel acorda exatamente a thread esperando há mais tempo naquela variável de condição

#### Scenario: Notificar todas acorda cada thread em espera
- **WHEN** um processo em modo usuário invoca a chamada de notificar todas, com o `handle` de uma variável de condição com uma ou mais threads esperando
- **THEN** o kernel acorda todas as threads atualmente esperando naquela variável de condição

#### Scenario: Handle desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca qualquer uma das chamadas de notificação com um `handle` que não corresponde a nenhuma variável de condição criada
- **THEN** o kernel devolve o sentinela de erro

### Requirement: Chamada de Consulta do Relógio de Parede
O kernel SHALL fornecer uma chamada de sistema que devolve ao chamador em modo usuário o valor atual do relógio de parede, em nanossegundos desde a época Unix.

#### Scenario: Consulta devolve o relógio de parede atual
- **WHEN** um processo em modo usuário invoca a chamada
- **THEN** o kernel devolve a data/hora atual em nanossegundos desde a época Unix

### Requirement: Chamada de Consulta do Relógio Monotônico
O kernel SHALL fornecer uma chamada de sistema que devolve ao chamador em modo usuário o valor atual do relógio monotônico, em nanossegundos.

#### Scenario: Consulta devolve o relógio monotônico atual
- **WHEN** um processo em modo usuário invoca a chamada
- **THEN** o kernel devolve o contador monotônico atual, em nanossegundos, maior ou igual a qualquer valor devolvido por uma consulta anterior
