import { Entity, Column, PrimaryGeneratedColumn, CreateDateColumn } from 'typeorm';

@Entity()
export class Ride {
  @PrimaryGeneratedColumn()
  id: number;

  @Column()
  pickup: string;

  @Column()
  dropoff: string;

  @Column({ default: 'pending' })
  status: string;

  @Column({ type: 'float', nullable: true })
  pickupLat: number;

  @Column({ type: 'float', nullable: true })
  pickupLng: number;

  @CreateDateColumn()
  createdAt: Date;
}