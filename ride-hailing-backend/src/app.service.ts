import { Injectable } from '@nestjs/common';
import { InjectRepository } from '@nestjs/typeorm';
import { Repository } from 'typeorm';
import { Ride } from './ride.entity';

@Injectable()
export class AppService {
  constructor(
    @InjectRepository(Ride)
    private rideRepository: Repository<Ride>,
  ) {}

  async createRide(pickup: string, dropoff: string): Promise<Ride> {
    const ride = this.rideRepository.create({ pickup, dropoff });
    return this.rideRepository.save(ride);
  }

  async getRides(): Promise<Ride[]> {
    return this.rideRepository.find();
  }
}