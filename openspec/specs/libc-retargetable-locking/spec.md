# libc-retargetable-locking Specification

## Purpose

Garante que os recursos internos do newlib usados por múltiplas threads do mesmo processo — heap de `malloc`, buffers de `stdio`, tabela de `atexit`, cache de ambiente/timezone — são protegidos por exclusão mútua real, via a API de locking retargetável do próprio newlib, em vez de ficarem sujeitos a corrupção silenciosa sob acesso concorrente.

## Requirements

### Requirement: Exclusão Mútua Real nos Recursos Internos do Newlib
Quando duas ou mais threads do mesmo processo acessam concorrentemente um
recurso interno do newlib protegido por sua API de *locking* retargetável
(heap de `malloc`, buffers de `stdio`, tabela de `atexit`, cache de
ambiente/timezone), o acesso a esse recurso SHALL ser serializado por
exclusão mútua real, nunca permitindo que duas threads o modifiquem ao
mesmo tempo.

#### Scenario: malloc/free concorrentes de duas threads nunca corrompem o heap
- **WHEN** duas threads do mesmo processo chamam `malloc`/`free`
  concorrentemente, repetidas vezes
- **THEN** toda alocação bem-sucedida SHALL continuar resultando numa
  região de memória utilizável, distinta de qualquer outra alocação viva,
  sem nenhuma corrupção observável do heap

#### Scenario: Acesso concorrente não depende de qual thread chega primeiro
- **WHEN** duas threads do mesmo processo tentam acessar o mesmo recurso
  interno protegido ao mesmo tempo
- **THEN** uma delas SHALL esperar a outra terminar antes de prosseguir,
  em vez de as duas operarem sobre o recurso simultaneamente

### Requirement: Reentrância Segura em Locks Recursivos
Uma thread que já detém um lock interno recursivo do newlib (heap de
`malloc`, buffers de `stdio`, tabela de `atexit`, cache de ambiente) SHALL
poder adquiri-lo novamente sem bloquear a si mesma, e esse lock só SHALL
ficar livre para outra thread depois de ser liberado o mesmo número de
vezes que foi adquirido.

#### Scenario: A mesma thread adquire o mesmo lock recursivo duas vezes sem travar
- **WHEN** uma thread adquire um lock interno recursivo que ela mesma já
  detém
- **THEN** essa segunda aquisição SHALL ter sucesso imediatamente, sem
  bloquear a thread chamadora

#### Scenario: Lock recursivo só fica livre após ser liberado o mesmo número de vezes
- **WHEN** uma thread adquire um lock interno recursivo duas vezes e o
  libera apenas uma vez
- **THEN** esse lock SHALL continuar detido por ela, e outra thread que
  tente adquiri-lo SHALL continuar bloqueada até a liberação
  correspondente à segunda aquisição

### Requirement: Threads de um Único Processo Não São Afetadas
Um processo de modo usuário com uma única thread SHALL continuar se
comportando exatamente como antes desta capacidade existir — nenhuma
mudança de comportamento observável em nenhum programa que nunca cria uma
segunda thread.

#### Scenario: Programa de uma única thread não observa nenhuma mudança
- **WHEN** um programa de modo usuário com uma única thread chama
  `malloc`/`free`, funções de `stdio`, `atexit`, ou qualquer outro recurso
  interno do newlib protegido por esta capacidade
- **THEN** o comportamento observável SHALL ser idêntico ao de antes desta
  capacidade existir
