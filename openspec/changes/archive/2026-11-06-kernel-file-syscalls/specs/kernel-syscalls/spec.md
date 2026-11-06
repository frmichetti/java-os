# Spec Delta

## ADDED Requirements

### Requirement: Chamada de Abertura de Arquivo
O kernel SHALL fornecer uma chamada de sistema que abre, por caminho absoluto fornecido pelo chamador, um arquivo do sistema de arquivos montado, devolvendo um `handle` opaco utilizável pelas chamadas de leitura e fechamento.

#### Scenario: Abertura bem-sucedida devolve um handle utilizável
- **WHEN** um processo em modo usuário invoca a chamada de abertura com um caminho absoluto válido apontando para um arquivo existente
- **THEN** o kernel devolve um `handle` que identifica esse arquivo aberto, pronto para leitura a partir do início

#### Scenario: Caminho inexistente ou apontando para diretório é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada de abertura com um caminho que não corresponde a nenhum arquivo existente, ou que corresponde a um diretório
- **THEN** o kernel devolve o sentinela de erro, sem produzir nenhum handle

### Requirement: Chamada de Leitura de Arquivo Aberto
O kernel SHALL fornecer uma chamada de sistema que lê, a partir do cursor atual do `handle` informado, até uma quantidade de bytes pedida pelo chamador, copiando-os para um buffer fornecido pelo chamador em modo usuário e avançando o cursor pela quantidade efetivamente lida.

#### Scenario: Leitura bem-sucedida copia os bytes para o buffer do chamador e avança o cursor
- **WHEN** um processo em modo usuário invoca a chamada de leitura com o `handle` de um arquivo aberto, um buffer válido e uma quantidade de bytes pedida
- **THEN** o kernel copia para o buffer do chamador a quantidade de bytes efetivamente lida (até a quantidade pedida, ou menos se o fim do arquivo for alcançado antes), devolve essa quantidade, e o cursor do handle avança na mesma quantidade

#### Scenario: Leitura no fim do arquivo devolve zero sem erro
- **WHEN** um processo em modo usuário invoca a chamada de leitura com o `handle` de um arquivo cujo cursor já está no fim do arquivo
- **THEN** o kernel devolve zero, sem copiar nenhum byte e sem tratar isso como erro

#### Scenario: Handle desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada de leitura com um `handle` que não corresponde a nenhum arquivo atualmente aberto
- **THEN** o kernel devolve o sentinela de erro, sem copiar nenhum byte

### Requirement: Chamada de Fechamento de Arquivo Aberto
O kernel SHALL fornecer uma chamada de sistema que fecha o `handle` de arquivo informado, liberando os recursos associados a ele.

#### Scenario: Fechamento bem-sucedido invalida o handle
- **WHEN** um processo em modo usuário invoca a chamada de fechamento com o `handle` de um arquivo atualmente aberto
- **THEN** o kernel libera os recursos associados a esse handle, e uma chamada de leitura subsequente com o mesmo handle é rejeitada como handle desconhecido

#### Scenario: Handle desconhecido é rejeitado
- **WHEN** um processo em modo usuário invoca a chamada de fechamento com um `handle` que não corresponde a nenhum arquivo atualmente aberto
- **THEN** o kernel devolve o sentinela de erro, sem afetar nenhum outro handle aberto
