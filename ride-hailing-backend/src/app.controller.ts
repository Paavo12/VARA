import { Controller, Get, Post, Body } from '@nestjs/common';
import { AppService } from './app.service';

@Controller()
export class AppController {
  constructor(private readonly appService: AppService) {}

  @Get()
  getHello(): string {
    return 'Ride demo API is running!';
  }

  @Post('ride')
  async createRide(
    @Body() body: { pickup: string; dropoff: string },
  ) {
    return this.appService.createRide(body.pickup, body.dropoff);
  }

  @Get('rides')
  async getRides() {
    return this.appService.getRides();
  }
}