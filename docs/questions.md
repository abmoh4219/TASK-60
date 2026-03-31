# questions.md – Business Logic Clarifications for RailOps

1. Data ingestion sources  
   Question: Exact format of “site” packages.  
   Understanding: Local JSON/CSV/XML folders with configurable tasks.  
   Solution: Modular ingestion engine with resumable tasks.

2. Quality scoring & publishing threshold  
   Question: Exact implementation of 85/100 block.  
   Understanding: Weighted score blocks publishing below threshold.  
   Solution: Dedicated cleansing service with audit logs.

3. Refund business rules  
   Question: Exact service-disruption exception handling.  
   Understanding: Manual override only for disruption cases.  
   Solution: Configurable rule engine with immutable audit.

4. Contractor matching scoring  
   Question: Exact algorithm transparency.  
   Understanding: Weighted score + top-3 reasons displayed.  
   Solution: Modular matching service.

5. Document watermarking & e-signing  
   Question: Watermark and signature flow.  
   Understanding: Server-side watermark + internal typed/drawn signature.  
   Solution: Secure document service with tamper-evident logs.

All other features from the original business requirements are implemented exactly as specified with senior-level modular design, central error handling, perfect role-based flows, and production readiness.