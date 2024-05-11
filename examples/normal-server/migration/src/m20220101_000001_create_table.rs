use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    #[allow(unused_labels)]
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        'DataTransaction: {
            manager
                .create_table(
                    Table::create()
                        .table(DataTransaction::Table)
                        .col(
                            ColumnDef::new(DataTransaction::Id)
                                .big_integer()
                                .not_null()
                                .auto_increment()
                                .primary_key(),
                        )
                        .col(
                            ColumnDef::new(DataTransaction::StartTime)
                                .timestamp()
                                .not_null(),
                        )
                        .to_owned(),
                )
                .await?;

            manager
                .create_index(
                    Index::create()
                        .name("IDX_start_time")
                        .table(DataTransaction::Table)
                        .col(DataTransaction::StartTime)
                        .to_owned(),
                )
                .await?;
        }

        'Data: {
            manager
                .create_table(
                    Table::create()
                        .table(Data::Table)
                        .col(
                            ColumnDef::new(Data::DataTransactionId)
                                .big_integer()
                                .not_null(),
                        )
                        .col(ColumnDef::new(Data::Id).big_integer().not_null())
                        .primary_key(
                            // Composite primary key
                            Index::create().col(Data::DataTransactionId).col(Data::Id),
                        )
                        .foreign_key(
                            ForeignKey::create()
                                .name("FK_data_transaction_id")
                                .from(Data::Table, Data::DataTransactionId)
                                .to(DataTransaction::Table, DataTransaction::Id),
                        )
                        .col(ColumnDef::new(Data::Ecg1).integer().not_null())
                        .col(ColumnDef::new(Data::Ecg2).integer().not_null())
                        .col(ColumnDef::new(Data::Quaternion).json().not_null())
                        .col(ColumnDef::new(Data::Accel).json().not_null())
                        .to_owned(),
                )
                .await?;
        }

        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .drop_table(Table::drop().table(Data::Table).to_owned())
            .await?;

        manager
            .drop_table(Table::drop().table(DataTransaction::Table).to_owned())
            .await?;

        Ok(())
    }
}

#[derive(DeriveIden)]
enum DataTransaction {
    Table,
    Id,
    StartTime,
}

#[derive(DeriveIden)]
enum Data {
    Table,
    Id,
    DataTransactionId,
    Ecg1,
    Ecg2,
    Quaternion,
    Accel,
}
